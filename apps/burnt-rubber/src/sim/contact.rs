//! **What happens when you hit something.** Severity, contact episodes,
//! separation, and the recovery assist.
//!
//! [`super::collision`] answers *are these two boxes overlapping, and where*.
//! This module answers everything after that: how bad it was, whether it is a
//! new collision or the same one still in progress, how much momentum it may
//! take, which way the car is shoved, and how the car is helped back onto its
//! line afterwards. Keeping the two apart is the whole point — the geometry is
//! about the world, the response is about the *game*, and only the response has
//! opinions.
//!
//! # The bug this module exists to fix
//!
//! A collision used to be a **state**, not an event. `resolve_traffic` ran once
//! per traffic car per fixed step, and its response was unconditional: if the
//! boxes still overlapped, the full response fired again. So a single mistake
//! compounded at 60 Hz.
//!
//! Rear-ending a 28 m/s car at 85 m/s, with the old `traffic_speed_keep` of
//! `0.58`, went `85 → 49 → 29 → 17` over three consecutive steps — three
//! separate thuds, three camera kicks, and 80% of the player's speed gone in
//! fifty milliseconds, from *one* mistake. Grinding a barrier or riding
//! alongside a car did the same thing indefinitely. That is not a difficult
//! collision, it is the game confiscating the car.
//!
//! The fix is structural rather than a smaller constant: a contact is an
//! **episode** with an identity ([`Obstacle`]), and an episode gets exactly one
//! full response. While the episode runs, the pair is still pushed apart and a
//! rate-limited scrape cue still plays — you can see and hear that you are
//! rubbing along something — but no further momentum is taken, no further camera
//! impulse is armed, and no further thud is scheduled.
//!
//! # Determinism
//!
//! Nothing here reads a clock or a random source. Episodes are counted in fixed
//! steps, severity is a pure function of contact geometry and velocity, and the
//! feedback amplitudes are pure functions of severity. The same commands produce
//! the same collisions, the same sounds and the same camera kicks.

use axiom_math::Vec3;

use crate::track::shortest_angle;
use crate::tuning::{CollisionTuning, DT};

use super::car::CarState;

/// How bad a contact was. Exactly three outcomes, deliberately: a player can
/// learn three, and every piece of feedback in the game — speed loss, deflection,
/// sound, camera, sparks — is keyed off this one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// A shallow rub. Barely interrupts the line.
    Scrape,
    /// An ordinary rear-end or side impact. Noticeable, survivable, recoverable.
    Bump,
    /// A genuinely severe event: square, fast, or into something immovable.
    MajorCrash,
}

impl Severity {
    /// The fraction of pre-impact forward speed this severity must leave behind.
    pub fn speed_floor(self, tuning: &CollisionTuning) -> f32 {
        match self {
            Severity::Scrape => tuning.scrape_speed_floor,
            Severity::Bump => tuning.bump_speed_floor,
            Severity::MajorCrash => tuning.crash_speed_floor,
        }
    }

    /// The lateral separation impulse (m/s) this severity applies.
    pub fn deflect(self, tuning: &CollisionTuning) -> f32 {
        match self {
            Severity::Scrape => tuning.scrape_deflect,
            Severity::Bump => tuning.bump_deflect,
            Severity::MajorCrash => tuning.crash_deflect,
        }
    }

    /// The yaw disturbance (rad/s) this severity applies. A scrape applies none:
    /// swinging the nose is what turns a rub into a spin.
    pub fn yaw_kick(self, tuning: &CollisionTuning) -> f32 {
        match self {
            Severity::Scrape => 0.0,
            Severity::Bump => tuning.bump_yaw_kick,
            Severity::MajorCrash => tuning.crash_yaw_kick,
        }
    }

    /// The feedback impulse amplitude (`0..1`) for the camera and the sparks.
    pub fn pulse(self, tuning: &CollisionTuning) -> f32 {
        match self {
            Severity::Scrape => tuning.scrape_pulse,
            Severity::Bump => tuning.bump_pulse,
            Severity::MajorCrash => tuning.crash_pulse,
        }
    }

    /// Whether this severity starts the recovery assist. A scrape does not —
    /// there is nothing to recover from, and an assist that fires on every rub
    /// is an autopilot the player never asked for.
    pub const fn arms_recovery(self) -> bool {
        matches!(self, Severity::Bump | Severity::MajorCrash)
    }

    /// A stable index, for table-driven presentation.
    pub const fn index(self) -> usize {
        match self {
            Severity::Scrape => 0,
            Severity::Bump => 1,
            Severity::MajorCrash => 2,
        }
    }
}

/// What was hit — and therefore how firm it is, and what counts as "the same
/// contact still going on".
///
/// The `slot` in [`Obstacle::Traffic`] is the traffic **slot id**, not the pool
/// index. A pool entry is recycled through many slots over a run, so keying an
/// episode on the index would suppress a collision with a brand new car because
/// an unrelated one happened to have used the same array cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Obstacle {
    /// A traffic car, identified by its slot.
    Traffic { slot: u32 },
    /// A guardrail on an open section: firm, with a little give.
    Barrier,
    /// The wall of a [`crate::track::SectionKind::walled`] section — tunnel
    /// lining, canyon rock. No guardrail, no give.
    Scenery,
}

impl Obstacle {
    /// Whether this is traffic, which is what the HUD and the audio distinguish.
    pub const fn is_traffic(self) -> bool {
        matches!(self, Obstacle::Traffic { .. })
    }

    /// The normal closing speed at which a square contact with this obstacle
    /// becomes a [`Severity::MajorCrash`].
    pub fn crash_normal_speed(self, tuning: &CollisionTuning) -> f32 {
        match self {
            Obstacle::Traffic { .. } => tuning.crash_normal_speed,
            Obstacle::Barrier => tuning.barrier_crash_normal_speed,
            Obstacle::Scenery => tuning.scenery_crash_normal_speed,
        }
    }
}

/// The deterministic facts a contact is classified from.
///
/// Every field is measured, never sampled: there is no random input anywhere in
/// severity classification, which is what lets a replay produce the same crashes
/// with the same sounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactFacts {
    /// What was hit.
    pub obstacle: Obstacle,
    /// Unit world direction pointing **from the obstacle toward the player** —
    /// the direction the player has to move to get out of it.
    pub normal: Vec3,
    /// Unit world direction the player should be biased sideways along, to slide
    /// clear rather than keep grinding. For a side-swipe this is the normal; for
    /// a rear-end it is whichever way round the obstacle has more room.
    pub bias: Vec3,
    /// Closing speed along the normal (m/s), never negative. This is the single
    /// most important number in the classification.
    pub normal_speed: f32,
    /// The player's forward speed immediately before the contact (m/s).
    pub player_speed: f32,
    /// The obstacle's own speed along the course (m/s); `0` for anything fixed.
    pub obstacle_speed: f32,
    /// How square the hit was: `0` is sliding along it, `1` is straight into it.
    pub squareness: f32,
    /// Whether the player struck the obstacle's rear rather than its side.
    pub rear_hit: bool,
}

impl ContactFacts {
    /// Whether every measured value is usable. A contact built from a poisoned
    /// state is *ignored* rather than propagated — this is the one place a NaN
    /// could otherwise reach the car's velocity.
    pub fn is_finite(&self) -> bool {
        let vectors = [self.normal, self.bias];
        let scalars = [
            self.normal_speed,
            self.player_speed,
            self.obstacle_speed,
            self.squareness,
        ];
        vectors
            .iter()
            .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite())
            && scalars.iter().all(|f| f.is_finite())
    }
}

/// Classify a contact into one of the three outcomes.
///
/// The rules, in the order they are decided:
///
/// * **Major crash** — a near-perpendicular hit above the obstacle's crash
///   closing speed, or ploughing square into something barely moving at real
///   speed. Both require squareness: you cannot have a severe crash sliding
///   along something.
/// * **Scrape** — shallow, *or* gentle along the normal. Either alone is enough;
///   a fast car brushing a wall at 5° is doing nothing violent, and neither is a
///   slow car nudging one square-on.
/// * **Bump** — everything in between, which is the ordinary case.
pub fn classify(facts: &ContactFacts, tuning: &CollisionTuning) -> Severity {
    let crash_normal = facts.obstacle.crash_normal_speed(tuning);
    // Driving square into something that is barely moving, fast. This is the
    // arm that makes a stopped car in your lane a genuine crash rather than an
    // ordinary shunt, without needing a separate obstacle kind for it.
    let ploughed = facts.obstacle_speed <= tuning.stationary_obstacle_speed
        && facts.player_speed >= tuning.stationary_crash_speed
        && facts.squareness >= tuning.crash_squareness;
    let severe = ploughed
        || (facts.normal_speed >= crash_normal && facts.squareness >= tuning.crash_squareness);
    // Shallow and gentle are separately sufficient, and both are exclusive with
    // `severe` by construction: `scrape_squareness < crash_squareness` and
    // `scrape_normal_speed < crash_normal_speed` are asserted in the tuning
    // tests, so the three bands cannot overlap however they are tuned.
    let shallow = facts.squareness <= tuning.scrape_squareness;
    let gentle = facts.normal_speed <= tuning.scrape_normal_speed;
    match (severe, shallow | gentle) {
        (true, _) => Severity::MajorCrash,
        (_, true) => Severity::Scrape,
        _ => Severity::Bump,
    }
}

/// What a resolved contact did, for the camera, the audio, the sparks and the
/// HUD. Presentation reads this and nothing else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Impact {
    /// World direction the car was shoved, unit.
    pub direction: Vec3,
    /// How bad it was.
    pub severity: Severity,
    /// `0..1`, where `1` is a square hit at the boosted top speed. Presentation
    /// scales *within* a severity's band with this; it never crosses bands.
    pub strength: f32,
    /// The feedback impulse amplitude (`0..1`). Zero on a sustained scrape cue,
    /// which is what keeps the camera from being re-kicked while grinding.
    pub pulse: f32,
    /// Whether the obstacle was traffic rather than fixed.
    pub traffic: bool,
    /// Whether this is the *opening* response of a contact episode, rather than
    /// a rate-limited cue from an episode already in progress.
    pub fresh: bool,
}

/// One contact episode: a single collision, for as long as it lasts.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Episode {
    obstacle: Obstacle,
    /// Fixed steps before this obstacle may deliver a full response again.
    steps_left: u32,
    /// Steps before the next rate-limited scrape cue.
    cue_left: u32,
    /// Whether the pair has genuinely come apart since the response, which ends
    /// the episode early: colliding again after separating is a new collision.
    cleared: bool,
}

/// The recovery assist: a short, fading helping hand after a real impact.
///
/// It has **two halves with different lifetimes**, and conflating them was a
/// bug worth naming. *Stabilisation* — damping the slide, damping the yaw kick,
/// biasing the heading back to the line — is finished the moment the car is
/// steady again, and holding it on past that is slop the player can feel.
/// *Momentum restoration* — the extra throttle — is not finished then at all:
/// the car stops wobbling almost immediately after an ordinary bump and is
/// still fifteen percent down on speed, which is precisely the thing the assist
/// exists to give back. One counter for both meant the early exit switched off
/// the acceleration a step after the impact, and the brief's "back to speed in
/// about a second" quietly stopped being helped by anything.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Recovery {
    /// Steps left on the acceleration assist. Runs its full course.
    steps_left: u32,
    /// Steps left on the stabilisation assist. Cut short once the car is
    /// steady.
    stabilise_left: u32,
    total: u32,
}

impl Recovery {
    /// The acceleration assist, `1` at the impact fading continuously to `0`.
    fn assist(self) -> f32 {
        (self.steps_left as f32 / self.total.max(1) as f32).clamp(0.0, 1.0)
    }

    /// The stabilisation assist, which fades on the same curve but can end
    /// early.
    fn stabilise(self) -> f32 {
        (self.stabilise_left as f32 / self.total.max(1) as f32).clamp(0.0, 1.0)
    }
}

/// Live contact state: which episodes are running, whether the car is
/// recovering, and the retained-momentum baseline for the current step.
///
/// Owned by [`super::RaceSim`] and threaded into the controller, because barrier
/// contacts are resolved *inside* the position integration and must consult the
/// same episode ledger as traffic contacts. Two ledgers would be two different
/// answers to "have I already been hit by this".
#[derive(Debug, Clone, PartialEq)]
pub struct ContactState {
    episodes: Vec<Episode>,
    recovery: Recovery,
    /// The player's forward speed before the *first* contact of this step, and
    /// the lowest severity floor any contact this step demanded. Together these
    /// are the retained-momentum rule: several contacts in one step clamp
    /// against one baseline, so they cannot compound.
    forward_before: Option<f32>,
    floor_this_step: f32,
}

/// Hard cap on live episodes. The pool of traffic plus the barrier is far below
/// it; the cap exists so a pathological state cannot grow the list without
/// bound, not because the game is expected to reach it.
const MAX_EPISODES: usize = 24;

impl ContactState {
    /// No contacts, no recovery.
    pub fn new() -> ContactState {
        ContactState {
            episodes: Vec::new(),
            recovery: Recovery::default(),
            forward_before: None,
            floor_this_step: 1.0,
        }
    }

    /// Forget everything — a restart, a reset, or a teleport, after any of which
    /// "the same car I was just touching" is meaningless.
    pub fn clear(&mut self) {
        self.episodes.clear();
        self.recovery = Recovery::default();
        self.forward_before = None;
        self.floor_this_step = 1.0;
    }

    /// How many episodes are live. Bounded by [`MAX_EPISODES`].
    pub fn episode_count(&self) -> usize {
        self.episodes.len()
    }

    /// Whether `obstacle` currently has an episode running, i.e. whether another
    /// full response against it would be suppressed.
    pub fn is_suppressed(&self, obstacle: Obstacle) -> bool {
        self.episodes
            .iter()
            .any(|e| e.obstacle == obstacle && e.steps_left > 0 && !e.cleared)
    }

    /// Whether the recovery assist is running.
    pub fn is_recovering(&self) -> bool {
        self.recovery.steps_left > 0
    }

    /// The **acceleration** assist, `1` immediately after an impact fading
    /// continuously to `0` over [`CollisionTuning::recovery_steps`]. This is the
    /// half that gives the lost momentum back, and it always runs its course.
    pub fn recovery_assist(&self) -> f32 {
        self.recovery.assist()
    }

    /// The **stabilisation** assist — the slide damping, the yaw damping and
    /// the heading bias. Fades on the same curve as [`Self::recovery_assist`]
    /// but drops to zero the moment the car is steady again, because an assist
    /// still nudging a settled car is an assist the player can feel fighting
    /// them.
    pub fn stabilise_assist(&self) -> f32 {
        self.recovery.stabilise()
    }

    /// Resolve a contact and return what presentation should do about it.
    ///
    /// `None` means the contact was fully suppressed: the pair is still being
    /// pushed apart by the caller, but nothing is taken from the player and
    /// nothing is played.
    pub fn respond(
        &mut self,
        car: &mut CarState,
        facts: &ContactFacts,
        tuning: &CollisionTuning,
    ) -> Option<Impact> {
        // A poisoned contact is dropped rather than applied. Nothing downstream
        // has to defend itself against a NaN normal because one never arrives.
        if !facts.is_finite() || !car.is_finite() {
            return None;
        }
        let severity = classify(facts, tuning);
        let strength = self.strength_of(facts, car);

        match self.slot_for(facts.obstacle) {
            // An episode is already running against this obstacle: no momentum
            // is taken, no camera impulse is armed. A scrape cue still goes out
            // on a fixed cadence so a sustained grind is audible and throws
            // sparks — that is the *only* thing a suppressed contact produces.
            Some(index) => {
                let episode = &mut self.episodes[index];
                episode.cue_left = episode.cue_left.saturating_sub(1);
                (episode.cue_left == 0).then(|| {
                    episode.cue_left = tuning.scrape_repeat_steps.max(1);
                    Impact {
                        direction: facts.normal,
                        severity: Severity::Scrape,
                        strength: strength.min(SUSTAINED_STRENGTH_CEILING),
                        pulse: 0.0,
                        traffic: facts.obstacle.is_traffic(),
                        fresh: false,
                    }
                })
            }
            None => Some(self.open_episode(car, facts, severity, strength, tuning)),
        }
    }

    /// Note how far apart the pair now are, ending the episode early once they
    /// have genuinely come apart. Colliding again after that is a new collision,
    /// not the same one — which is what stops a cooldown from turning the player
    /// briefly intangible.
    pub fn note_gap(&mut self, obstacle: Obstacle, gap: f32, tuning: &CollisionTuning) {
        let cleared = gap >= tuning.separation_clearance;
        self.episodes
            .iter_mut()
            .filter(|e| e.obstacle == obstacle)
            .for_each(|e| e.cleared |= cleared);
    }

    /// End the fixed step: age every episode, fade the recovery, decay the
    /// collision's yaw disturbance, and drop the retained-momentum baseline.
    pub fn advance(&mut self, car: &mut CarState, tuning: &CollisionTuning) {
        self.episodes.iter_mut().for_each(|e| {
            e.steps_left = e.steps_left.saturating_sub(1);
        });
        self.episodes.retain(|e| e.steps_left > 0 && !e.cleared);
        self.recovery.steps_left = self.recovery.steps_left.saturating_sub(1);
        self.recovery.stabilise_left = self.recovery.stabilise_left.saturating_sub(1);

        // The collision's own yaw disturbance decays on its own, faster while
        // the recovery assist is running. It is stored on the car rather than
        // applied as a one-shot `yaw +=` precisely so that it *can* be damped:
        // a disturbance you cannot damp is a disturbance recovery cannot help
        // with.
        let damping = tuning.impact_yaw_decay + tuning.recovery_yaw_damp * self.stabilise_assist();
        car.impact_yaw_rate *= (-damping * DT).exp();
        if car.impact_yaw_rate.abs() <= IMPACT_YAW_EPSILON {
            car.impact_yaw_rate = 0.0;
        }

        // Stop *stabilising* early once the car is genuinely settled — an assist
        // that keeps nudging a stable car is an assist the player can feel. The
        // acceleration half keeps running: the car has stopped wobbling, it has
        // not got its speed back.
        let settled = car.lateral_speed.abs() < tuning.recovery_stable_lateral
            && car.impact_yaw_rate.abs() < tuning.recovery_stable_yaw;
        if settled {
            self.recovery.stabilise_left = 0;
        }

        self.forward_before = None;
        self.floor_this_step = 1.0;
    }

    /// Open a fresh episode: this is the one place momentum is taken.
    fn open_episode(
        &mut self,
        car: &mut CarState,
        facts: &ContactFacts,
        severity: Severity,
        strength: f32,
        tuning: &CollisionTuning,
    ) -> Impact {
        let floor = severity.speed_floor(tuning);
        // The baseline is the speed before the FIRST contact of this step, so
        // two cars hit in one step clamp against the same number instead of
        // each taking its cut of what the previous one left.
        let baseline = *self.forward_before.get_or_insert(car.forward_speed);
        self.floor_this_step = self.floor_this_step.min(floor);

        // The loss ramps in with the closing speed and caps at exactly the
        // floor's complement, so the cap and the floor can never disagree.
        let ramp = (facts.normal_speed / tuning.loss_reference_speed.max(1.0e-3)).clamp(0.0, 1.0);
        let loss = tuning.max_loss(floor) * ramp;
        car.forward_speed *= 1.0 - loss;
        self.clamp_retained(car, baseline);

        // The deflection is purely lateral, by design. Pushing a rear-ended car
        // *backwards* along its own nose would fight the retained-momentum floor
        // it was just clamped against; the along-course share of a contact is
        // separation's job, not the response's.
        let sideways = facts.bias.dot(car.right());
        let deflect_sign = if sideways >= 0.0 { 1.0 } else { -1.0 };
        car.lateral_speed += deflect_sign * severity.deflect(tuning) * strength.max(MIN_DEFLECT);

        // Increasing yaw is a turn the player sees as LEFT (see
        // `controller::rotate_chassis`), and `car.right()` is the simulation's
        // right, so being shoved to the right must *decrease* yaw for the nose
        // to follow the shove.
        car.impact_yaw_rate += -deflect_sign * severity.yaw_kick(tuning) * strength.max(MIN_DEFLECT);

        let pulse = severity.pulse(tuning) * strength.max(MIN_PULSE);
        car.impact_direction = facts.normal.normalize().unwrap_or(Vec3::UNIT_Z);
        car.impact_strength = car.impact_strength.max(pulse);
        car.impact_steps = car
            .impact_steps
            .max((pulse * IMPACT_STEP_SCALE) as u32 + IMPACT_STEP_FLOOR);

        if severity.arms_recovery() {
            self.recovery = Recovery {
                steps_left: tuning.recovery_steps,
                stabilise_left: tuning.recovery_steps,
                total: tuning.recovery_steps,
            };
        }

        // Bounded: an episode list that cannot grow is one fewer thing a long
        // run can go wrong at. The oldest entry is displaced, which in practice
        // never happens — the pool of traffic is a third of the cap.
        if self.episodes.len() >= MAX_EPISODES {
            self.episodes.remove(0);
        }
        self.episodes.push(Episode {
            obstacle: facts.obstacle,
            steps_left: tuning.episode_steps.max(1),
            cue_left: tuning.scrape_repeat_steps.max(1),
            cleared: false,
        });

        Impact {
            direction: car.impact_direction,
            severity,
            strength,
            pulse,
            traffic: facts.obstacle.is_traffic(),
            fresh: true,
        }
    }

    /// The **retained-momentum rule**, in one place: whatever a collision
    /// computed, the car leaves it with at least `floor` of the forward speed it
    /// arrived with. Only ever raises the speed back up, never lowers it, and
    /// never touches a reversing car (where the floor would mean the opposite of
    /// what it says).
    fn clamp_retained(&self, car: &mut CarState, baseline: f32) {
        let floored = baseline * self.floor_this_step;
        let applies = baseline > 0.0 && floored.is_finite();
        if applies {
            car.forward_speed = car.forward_speed.max(floored);
        }
    }

    /// The index of a live, un-cleared episode against `obstacle`.
    fn slot_for(&self, obstacle: Obstacle) -> Option<usize> {
        self.episodes
            .iter()
            .position(|e| e.obstacle == obstacle && e.steps_left > 0 && !e.cleared)
    }

    /// How hard the hit was, `0..1`, measured against the fastest the car can
    /// ever be going. This scales feedback *within* a severity band; it never
    /// decides the band.
    fn strength_of(&self, facts: &ContactFacts, car: &CarState) -> f32 {
        let ceiling = facts.player_speed.abs().max(car.speed()).max(STRENGTH_FLOOR);
        (facts.normal_speed / ceiling).clamp(0.0, 1.0)
    }
}

impl Default for ContactState {
    fn default() -> Self {
        ContactState::new()
    }
}

/// Speed (m/s) the impact strength is measured against when the car is barely
/// moving, so a nudge at walking pace does not read as a full-strength hit.
const STRENGTH_FLOOR: f32 = 30.0;

/// Least fraction of a severity's deflection and yaw kick any qualifying contact
/// gets. Without a floor, a contact that only just crossed into "bump" would be
/// classified as one and then feel like nothing.
const MIN_DEFLECT: f32 = 0.35;

/// Least fraction of a severity's feedback pulse, for the same reason.
const MIN_PULSE: f32 = 0.5;

/// Ceiling on the reported strength of a *sustained* scrape cue. A grind is
/// quiet however fast you are going along it.
const SUSTAINED_STRENGTH_CEILING: f32 = 0.28;

/// Yaw disturbance (rad/s) below which the collision's kick is finished.
const IMPACT_YAW_EPSILON: f32 = 1.0e-3;

/// Steps of ringing impact state per unit of feedback pulse — what the sparks
/// burn for.
const IMPACT_STEP_SCALE: f32 = 34.0;
/// Minimum steps any registered impact rings for.
const IMPACT_STEP_FLOOR: u32 = 8;

/// The recovery assist's effect on the chassis heading: a gentle pull toward a
/// blend of where the car is actually going and where the road goes.
///
/// This is deliberately a *pull*, added to the yaw rate the player's steering
/// already produced, rather than a replacement for it. The player keeps the
/// wheel — the assist only removes the part of the disturbance they did not ask
/// for. Returns the extra yaw rate (rad/s) to apply this step.
pub fn recovery_heading_pull(
    car: &CarState,
    road_heading: f32,
    assist: f32,
    tuning: &CollisionTuning,
) -> f32 {
    let travel = car.heading_of_travel();
    let travel_yaw = travel.x.atan2(travel.z);
    // Blend the car's own direction of travel with the road's: pure travel would
    // lock in a slide that is heading off the road, pure road would drive for
    // you.
    let wanted = travel_yaw + shortest_angle(road_heading - travel_yaw) * tuning.recovery_road_blend;
    shortest_angle(wanted - car.yaw) * tuning.recovery_heading_pull * assist
}

/// The recovery assist's extra lateral bleed, applied to the slide **above** the
/// stable threshold only.
///
/// Damping the whole slide would delete the deflection the collision just
/// applied, which is the readable part of the feedback. Damping only the excess
/// keeps the shove and removes the spin.
pub fn recovery_damp_lateral(car: &mut CarState, assist: f32, tuning: &CollisionTuning) {
    // Not an optimisation. Reconstructing the slide as `stable + excess` is
    // algebraically the identity when `assist` is zero, but it is not the
    // identity in *floating point* — and a car that is not recovering from
    // anything must be bit-for-bit the car it would have been before this
    // function existed, or every handling number in the game shifts by a hair.
    if assist <= 0.0 {
        return;
    }
    let magnitude = car.lateral_speed.abs();
    let excess = (magnitude - tuning.recovery_stable_lateral).max(0.0);
    let damped = excess * (-tuning.recovery_lateral_damp * assist * DT).exp();
    let wanted = tuning.recovery_stable_lateral.min(magnitude) + damped;
    car.lateral_speed = car.lateral_speed.signum() * wanted;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::CollisionTuning;

    fn facts(normal_speed: f32, squareness: f32) -> ContactFacts {
        ContactFacts {
            obstacle: Obstacle::Traffic { slot: 7 },
            normal: Vec3::UNIT_X,
            bias: Vec3::UNIT_X,
            normal_speed,
            player_speed: 80.0,
            obstacle_speed: 30.0,
            squareness,
            rear_hit: false,
        }
    }

    fn car_at(speed: f32) -> CarState {
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        car.forward_speed = speed;
        car
    }

    #[test]
    fn a_shallow_contact_is_a_scrape_however_fast_it_was() {
        let t = CollisionTuning::DEFAULT;
        assert_eq!(classify(&facts(40.0, 0.05), &t), Severity::Scrape);
        assert_eq!(classify(&facts(4.0, 0.9), &t), Severity::Scrape, "and so is a gentle one");
    }

    #[test]
    fn an_ordinary_contact_is_a_bump() {
        let t = CollisionTuning::DEFAULT;
        assert_eq!(classify(&facts(16.0, 0.5), &t), Severity::Bump);
        assert_eq!(classify(&facts(30.0, 0.5), &t), Severity::Bump, "fast but not square");
        assert_eq!(classify(&facts(14.0, 0.95), &t), Severity::Bump, "square but not fast");
    }

    #[test]
    fn a_fast_square_contact_is_a_major_crash() {
        let t = CollisionTuning::DEFAULT;
        assert_eq!(classify(&facts(35.0, 0.95), &t), Severity::MajorCrash);
    }

    #[test]
    fn ploughing_into_something_stationary_at_speed_is_a_major_crash() {
        let t = CollisionTuning::DEFAULT;
        let mut f = facts(18.0, 0.9);
        f.obstacle_speed = 0.0;
        f.player_speed = 80.0;
        assert_eq!(
            classify(&f, &t),
            Severity::MajorCrash,
            "below the crash closing speed, but square into a standing obstacle at 80 m/s"
        );
        // The same geometry against something that is genuinely moving is not.
        f.obstacle_speed = 30.0;
        assert_eq!(classify(&f, &t), Severity::Bump);
    }

    #[test]
    fn the_firmness_of_the_obstacle_moves_the_crash_threshold() {
        let t = CollisionTuning::DEFAULT;
        let square = 0.9;
        let speed = 16.0;
        let obstacles = [
            (Obstacle::Traffic { slot: 1 }, Severity::Bump),
            (Obstacle::Barrier, Severity::Bump),
            (Obstacle::Scenery, Severity::MajorCrash),
        ];
        for (obstacle, expected) in obstacles {
            let mut f = facts(speed, square);
            f.obstacle = obstacle;
            f.obstacle_speed = 0.0;
            f.player_speed = 40.0; // below the ploughing threshold
            assert_eq!(classify(&f, &t), expected, "{obstacle:?} at {speed} m/s");
        }
    }

    #[test]
    fn traffic_is_the_only_obstacle_reported_as_traffic() {
        assert!(Obstacle::Traffic { slot: 0 }.is_traffic());
        assert!(!Obstacle::Barrier.is_traffic());
        assert!(!Obstacle::Scenery.is_traffic());
    }

    #[test]
    fn the_severity_ladder_reports_ordered_feedback() {
        let t = CollisionTuning::DEFAULT;
        let ladder = [Severity::Scrape, Severity::Bump, Severity::MajorCrash];
        let floors: Vec<f32> = ladder.iter().map(|s| s.speed_floor(&t)).collect();
        let deflects: Vec<f32> = ladder.iter().map(|s| s.deflect(&t)).collect();
        let pulses: Vec<f32> = ladder.iter().map(|s| s.pulse(&t)).collect();
        let kicks: Vec<f32> = ladder.iter().map(|s| s.yaw_kick(&t)).collect();
        assert!(floors[0] > floors[1] && floors[1] > floors[2], "{floors:?}");
        assert!(deflects[0] < deflects[1] && deflects[1] < deflects[2]);
        assert!(pulses[0] < pulses[1] && pulses[1] < pulses[2]);
        assert_eq!(kicks[0], 0.0, "a scrape never swings the nose");
        assert!(kicks[1] < kicks[2]);
        assert!(!Severity::Scrape.arms_recovery(), "and never starts a recovery");
        assert!(Severity::Bump.arms_recovery() && Severity::MajorCrash.arms_recovery());
        let indices: Vec<usize> = ladder.iter().map(|s| s.index()).collect();
        assert_eq!(indices, vec![0, 1, 2]);
        assert!(Severity::Scrape < Severity::MajorCrash, "and they order");
    }

    /// The headline promise, per severity, measured directly.
    #[test]
    fn each_severity_retains_at_least_its_floor_of_the_pre_impact_speed() {
        let t = CollisionTuning::DEFAULT;
        // Facts chosen to sit deep inside each band, at a closing speed well past
        // the loss reference so every severity takes its FULL cut.
        let cases = [
            (facts(2.0, 0.1), Severity::Scrape, t.scrape_speed_floor),
            (facts(20.0, 0.5), Severity::Bump, t.bump_speed_floor),
            (facts(60.0, 0.95), Severity::MajorCrash, t.crash_speed_floor),
        ];
        for (f, expected, floor) in cases {
            let mut state = ContactState::new();
            let mut car = car_at(90.0);
            let impact = state.respond(&mut car, &f, &t).expect("a fresh contact responds");
            assert_eq!(impact.severity, expected, "{f:?}");
            assert!(
                car.forward_speed >= 90.0 * floor - 1.0e-4,
                "{expected:?} left {} m/s of 90, below the {floor} floor",
                car.forward_speed
            );
            assert!(car.forward_speed < 90.0, "but it still cost something");
            assert!(car.is_finite());
        }
    }

    /// The bug, pinned: a sustained overlap must cost its momentum **once**.
    #[test]
    fn a_sustained_overlap_does_not_compound_the_speed_loss() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        let f = facts(30.0, 0.6);

        state.respond(&mut car, &f, &t).expect("the first contact responds");
        let after_first = car.forward_speed;
        assert!(after_first < 90.0);

        // Thirty more steps of the same overlap — half a second of grinding.
        for _ in 0..30 {
            state.advance(&mut car, &t);
            state.respond(&mut car, &f, &t);
        }
        assert!(
            car.forward_speed >= after_first - 1.0e-3,
            "the grind kept taking speed: {after_first} -> {}",
            car.forward_speed
        );
        assert!(
            car.forward_speed >= 90.0 * t.bump_speed_floor - 1.0e-3,
            "and the floor held across the whole episode: {}",
            car.forward_speed
        );
    }

    #[test]
    fn the_same_vehicle_cannot_trigger_a_second_full_impact_during_the_cooldown() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        let f = facts(30.0, 0.6);
        assert!(state.respond(&mut car, &f, &t).expect("first").fresh);
        assert!(state.is_suppressed(f.obstacle));

        let mut fresh_again = 0;
        for _ in 0..t.episode_steps - 1 {
            state.advance(&mut car, &t);
            let reported = state.respond(&mut car, &f, &t);
            fresh_again += usize::from(reported.is_some_and(|i| i.fresh));
        }
        assert_eq!(fresh_again, 0, "the cooldown held for its whole length");

        // And once it expires, a genuinely new impact lands again.
        state.advance(&mut car, &t);
        assert!(!state.is_suppressed(f.obstacle), "the episode is over");
        assert!(state.respond(&mut car, &f, &t).expect("a new episode").fresh);
    }

    #[test]
    fn a_different_vehicle_can_still_be_hit_during_the_cooldown() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        let first = facts(30.0, 0.6);
        state.respond(&mut car, &first, &t).expect("first");

        let mut other = facts(30.0, 0.6);
        other.obstacle = Obstacle::Traffic { slot: 8 };
        let hit = state.respond(&mut car, &other, &t).expect("an unrelated car");
        assert!(hit.fresh, "a different vehicle is a different collision");
        assert_eq!(state.episode_count(), 2, "two independent episodes");

        // And a barrier is independent of both.
        let mut wall = facts(30.0, 0.9);
        wall.obstacle = Obstacle::Barrier;
        assert!(state.respond(&mut car, &wall, &t).expect("the wall").fresh);
    }

    /// Two contacts in one step clamp against **one** baseline, so they cannot
    /// take their cut of each other's leftovers.
    #[test]
    fn several_contacts_in_one_step_clamp_against_a_single_baseline() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        for slot in 0..4u32 {
            let mut f = facts(30.0, 0.6);
            f.obstacle = Obstacle::Traffic { slot };
            state.respond(&mut car, &f, &t).expect("each is a fresh car");
        }
        assert!(
            car.forward_speed >= 90.0 * t.bump_speed_floor - 1.0e-4,
            "four bumps in one step still left {} m/s of 90",
            car.forward_speed
        );
    }

    /// Separating genuinely re-arms the collision — a cooldown must never make
    /// the player briefly intangible.
    #[test]
    fn coming_apart_ends_the_episode_early() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        let f = facts(30.0, 0.6);
        state.respond(&mut car, &f, &t).expect("first");
        assert!(state.is_suppressed(f.obstacle));

        // A gap smaller than the clearance is still the same contact.
        state.note_gap(f.obstacle, t.separation_clearance * 0.5, &t);
        assert!(state.is_suppressed(f.obstacle), "still rubbing along it");

        state.note_gap(f.obstacle, t.separation_clearance + 0.1, &t);
        assert!(!state.is_suppressed(f.obstacle), "genuinely apart");
        assert!(
            state.respond(&mut car, &f, &t).expect("re-contact").fresh,
            "hitting it again is a new collision"
        );
    }

    #[test]
    fn a_suppressed_contact_emits_rate_limited_scrape_cues_and_nothing_else() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        let f = facts(30.0, 0.6);
        state.respond(&mut car, &f, &t).expect("the opening hit");

        let mut cues = 0;
        for _ in 0..t.episode_steps - 1 {
            state.advance(&mut car, &t);
            if let Some(cue) = state.respond(&mut car, &f, &t) {
                assert!(!cue.fresh, "a cue is not a fresh impact");
                assert_eq!(cue.severity, Severity::Scrape, "a grind is always a scrape");
                assert_eq!(cue.pulse, 0.0, "and never re-kicks the camera");
                assert!(cue.strength <= SUSTAINED_STRENGTH_CEILING);
                cues += 1;
            }
        }
        // Audible and continuous, but nowhere near one per step.
        let steps = t.episode_steps - 1;
        assert!(cues >= 2, "a grind is audible: {cues} cues in {steps} steps");
        assert!(
            cues <= steps / t.scrape_repeat_steps + 1,
            "and rate limited: {cues} cues in {steps} steps"
        );
    }

    #[test]
    fn a_bump_arms_recovery_and_a_scrape_does_not() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        state.respond(&mut car, &facts(2.0, 0.1), &t).expect("a scrape");
        assert!(!state.is_recovering(), "a rub is not something to recover from");

        let mut other = facts(30.0, 0.6);
        other.obstacle = Obstacle::Traffic { slot: 99 };
        state.respond(&mut car, &other, &t).expect("a bump");
        assert!(state.is_recovering());
        assert!((state.recovery_assist() - 1.0).abs() < 1.0e-6, "at full strength");
    }

    #[test]
    fn the_recovery_assist_fades_continuously_to_nothing() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        // A big slide, so the "already stable" early exit does not fire.
        car.lateral_speed = 20.0;
        state.respond(&mut car, &facts(30.0, 0.6), &t).expect("a bump");

        let mut previous = state.recovery_assist();
        let mut samples = vec![previous];
        for _ in 0..t.recovery_steps {
            car.lateral_speed = 20.0;
            state.advance(&mut car, &t);
            let now = state.recovery_assist();
            assert!(now <= previous + 1.0e-6, "the fade never rises: {previous} -> {now}");
            assert!(previous - now < 0.05, "and never steps: {previous} -> {now}");
            previous = now;
            samples.push(now);
        }
        assert_eq!(previous, 0.0, "and it finishes");
        assert!(samples.iter().any(|a| (0.3..0.7).contains(a)), "genuinely gradual");
        assert!(!state.is_recovering());
    }

    /// Stabilisation stops early when the car is steady. The **acceleration**
    /// assist does not, and that distinction is the whole reason the two are
    /// separate counters: an ordinary bump stops the car wobbling within a step
    /// or two and leaves it fifteen percent down on speed, so an early exit that
    /// switched off the throttle help would switch it off exactly when it was
    /// needed and never when it was not.
    #[test]
    fn stabilisation_stops_early_when_steady_but_the_throttle_help_does_not() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        car.lateral_speed = 20.0;
        state.respond(&mut car, &facts(30.0, 0.6), &t).expect("a bump");
        assert!(state.stabilise_assist() > 0.0);

        // The car settles immediately: no slide, no yaw disturbance.
        car.lateral_speed = 0.0;
        car.impact_yaw_rate = 0.0;
        state.advance(&mut car, &t);
        assert_eq!(state.stabilise_assist(), 0.0, "nothing left to stabilise");
        assert!(
            state.is_recovering() && state.recovery_assist() > 0.5,
            "but the momentum is still owed back: {}",
            state.recovery_assist()
        );

        // And the throttle help runs its full course before finishing.
        for _ in 0..t.recovery_steps {
            state.advance(&mut car, &t);
        }
        assert_eq!(state.recovery_assist(), 0.0);
        assert!(!state.is_recovering());
    }

    /// Once stabilisation has ended, it stays ended for that recovery — an
    /// assist that flickers back on is one the player feels as the car
    /// twitching under them.
    #[test]
    fn stabilisation_does_not_come_back_once_it_has_ended() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        state.respond(&mut car, &facts(30.0, 0.6), &t).expect("a bump");
        car.lateral_speed = 0.0;
        car.impact_yaw_rate = 0.0;
        state.advance(&mut car, &t);
        assert_eq!(state.stabilise_assist(), 0.0);
        // A fresh slide, from the player's own driving rather than the impact.
        car.lateral_speed = 30.0;
        state.advance(&mut car, &t);
        assert_eq!(
            state.stabilise_assist(),
            0.0,
            "a slide the player caused is not the collision's to correct"
        );
    }

    #[test]
    fn the_yaw_disturbance_is_bounded_signed_and_decays_to_zero() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        let mut left = facts(30.0, 0.6);
        left.bias = Vec3::UNIT_X; // the simulation's right at yaw 0
        state.respond(&mut car, &left, &t).expect("a bump");
        let kicked = car.impact_yaw_rate;
        assert!(kicked < 0.0, "shoved right, the nose follows right (yaw falls)");
        assert!(kicked.abs() <= t.bump_yaw_kick + 1.0e-6, "and it is bounded");

        // The mirrored contact kicks the other way by the same amount.
        let mut mirrored = ContactState::new();
        let mut other = car_at(90.0);
        let mut right = left;
        right.bias = Vec3::new(-1.0, 0.0, 0.0);
        mirrored.respond(&mut other, &right, &t).expect("a bump");
        assert!((other.impact_yaw_rate + kicked).abs() < 1.0e-5, "symmetric");

        for _ in 0..180 {
            state.advance(&mut car, &t);
        }
        assert_eq!(car.impact_yaw_rate, 0.0, "and it finishes at exactly zero");
    }

    #[test]
    fn the_lateral_damp_trims_the_excess_and_keeps_the_deflection() {
        let t = CollisionTuning::DEFAULT;
        let mut car = car_at(60.0);
        car.lateral_speed = 18.0;
        for _ in 0..60 {
            recovery_damp_lateral(&mut car, 1.0, &t);
        }
        assert!(car.lateral_speed < 18.0, "the excess is trimmed");
        assert!(
            car.lateral_speed >= t.recovery_stable_lateral - 1.0e-4,
            "but the readable deflection survives: {}",
            car.lateral_speed
        );

        // A slide already inside the stable band is left completely alone.
        let mut settled = car_at(60.0);
        settled.lateral_speed = t.recovery_stable_lateral * 0.5;
        let before = settled.lateral_speed;
        recovery_damp_lateral(&mut settled, 1.0, &t);
        assert!((settled.lateral_speed - before).abs() < 1.0e-6);

        // And it is sign-preserving.
        let mut leftward = car_at(60.0);
        leftward.lateral_speed = -18.0;
        recovery_damp_lateral(&mut leftward, 1.0, &t);
        assert!(leftward.lateral_speed < 0.0, "a left slide stays a left slide");
        assert!(leftward.lateral_speed > -18.0);
    }

    #[test]
    fn the_heading_pull_aims_between_the_travel_direction_and_the_road() {
        let t = CollisionTuning::DEFAULT;
        let mut car = car_at(60.0);
        car.yaw = 0.6; // nose swung well off the road direction
        let pull = recovery_heading_pull(&car, 0.0, 1.0, &t);
        assert!(pull < 0.0, "the nose is pulled back down toward zero");
        assert!(
            pull.abs() <= 0.6 * t.recovery_heading_pull + 1.0e-6,
            "and the pull is proportional and bounded"
        );
        // It fades away with the assist, exactly like everything else.
        assert!(recovery_heading_pull(&car, 0.0, 0.5, &t).abs() < pull.abs());
        assert_eq!(recovery_heading_pull(&car, 0.0, 0.0, &t), 0.0);
        // A car already pointing down the road is left alone.
        car.yaw = 0.0;
        assert!(recovery_heading_pull(&car, 0.0, 1.0, &t).abs() < 1.0e-6);
    }

    #[test]
    fn a_poisoned_contact_is_dropped_rather_than_applied() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        let mut bad = facts(30.0, 0.6);
        bad.normal = Vec3::new(f32::NAN, 0.0, 0.0);
        assert!(!bad.is_finite());
        assert!(state.respond(&mut car, &bad, &t).is_none());
        assert_eq!(car.forward_speed, 90.0, "and the car is untouched");
        assert!(car.is_finite());

        // A poisoned CAR is refused too, rather than having a contact layered on.
        let mut poisoned = car_at(f32::NAN);
        assert!(state.respond(&mut poisoned, &facts(30.0, 0.6), &t).is_none());
    }

    #[test]
    fn a_degenerate_normal_falls_back_instead_of_producing_a_nan() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        let mut f = facts(30.0, 0.6);
        f.normal = Vec3::ZERO;
        f.bias = Vec3::ZERO;
        let impact = state.respond(&mut car, &f, &t).expect("still reported");
        assert_eq!(impact.direction, Vec3::UNIT_Z);
        assert!(car.is_finite());
    }

    #[test]
    fn a_reversing_car_is_not_given_speed_by_the_retained_momentum_floor() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(-8.0);
        state.respond(&mut car, &facts(30.0, 0.6), &t).expect("a bump");
        assert!(car.forward_speed <= 0.0, "still reversing: {}", car.forward_speed);
        assert!(car.forward_speed > -8.0 - 1.0e-4, "and no faster than it was");
        assert!(car.is_finite());
    }

    #[test]
    fn clearing_forgets_every_episode_and_the_recovery() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        state.respond(&mut car, &facts(30.0, 0.6), &t).expect("a bump");
        assert!(state.episode_count() > 0 && state.is_recovering());
        state.clear();
        assert_eq!(state.episode_count(), 0);
        assert!(!state.is_recovering());
        assert_eq!(state, ContactState::default());
    }

    #[test]
    fn the_episode_list_is_bounded_however_many_things_are_hit() {
        let t = CollisionTuning::DEFAULT;
        let mut state = ContactState::new();
        let mut car = car_at(90.0);
        for slot in 0..(MAX_EPISODES as u32 * 3) {
            let mut f = facts(30.0, 0.6);
            f.obstacle = Obstacle::Traffic { slot };
            state.respond(&mut car, &f, &t);
            assert!(state.episode_count() <= MAX_EPISODES, "{}", state.episode_count());
        }
        assert_eq!(state.episode_count(), MAX_EPISODES);
        // The most recent contacts are the ones that survived, which is the only
        // ordering that matters: an old episode being dropped is an old
        // collision being forgotten.
        assert!(state.is_suppressed(Obstacle::Traffic {
            slot: MAX_EPISODES as u32 * 3 - 1
        }));
    }

    #[test]
    fn contact_facts_reject_every_flavour_of_poison() {
        let mut f = facts(30.0, 0.6);
        assert!(f.is_finite());
        f.normal_speed = f32::INFINITY;
        assert!(!f.is_finite());
        let mut f = facts(30.0, 0.6);
        f.bias = Vec3::new(0.0, f32::NAN, 0.0);
        assert!(!f.is_finite());
        let mut f = facts(30.0, 0.6);
        f.squareness = f32::NAN;
        assert!(!f.is_finite());
    }
}
