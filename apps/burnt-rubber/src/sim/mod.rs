//! The deterministic race simulation: one fixed step, everything in it.
//!
//! [`RaceSim`] owns the course, the car, the traffic, the boost meter, the
//! camera and the run state, and advances all of them by exactly one 60 Hz step
//! per call to [`RaceSim::step`]. It reads a [`DriveCommand`] and nothing else —
//! no clock, no randomness beyond the seeded course and traffic streams, no
//! ambient globals. Given the same seed and the same ordered commands it
//! produces the same track, the same car state, the same traffic, the same boost
//! meter, the same collision events and the same progress. That is the property
//! the replay tests pin, and it is the reason presentation is kept strictly
//! downstream: nothing a renderer or a browser does can reach in here.
//!
//! The step order is fixed and matters:
//!
//! 1. resolve the one-shot commands (restart, reset, pause);
//! 2. ask the boost meter whether boost may be spent;
//! 3. drive the car (which resolves barriers inside its own sub-moves);
//! 4. award every boost pickup the car drove over on the way;
//! 5. advance the traffic;
//! 6. resolve traffic contacts, then award near misses on what is left;
//! 7. update progress, the finish, and the stuck detector;
//! 8. advance the camera over the settled state.
//!
//! Traffic contacts are resolved *before* near misses are awarded so a car you
//! actually hit can never also pay out as a near miss. Pickups sit outside that
//! ordering entirely, because they are not in competition with either: a pickup
//! is collected by having driven over it, and nothing that happens to the car
//! afterwards changes whether it did.

pub mod boost;
pub mod car;
pub mod chassis;
pub mod collision;
pub mod contact;
pub mod controller;
pub mod pickups;
pub mod rails;
pub mod traffic;

use std::sync::Arc;

use axiom_math::Vec3;

use crate::camera::{CameraPose, ChaseCamera, ImpactImpulse};
use crate::command::DriveCommand;
use crate::course::runtime::CoursePlan;
use crate::track::{SectionKind, Track, GRID_DISTANCE};
use crate::tuning::{Tuning, DT};

use boost::BoostMeter;
use car::{CarPose, CarState};
use contact::{ContactState, Severity};
use pickups::PickupField;
use traffic::Traffic;

use crate::course::specification::BoostTier;

/// What the run is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RacePhase {
    /// Counting the player in. The car is held.
    Countdown,
    /// Driving.
    Racing,
    /// Over the line. The car coasts to a stop; the camera keeps working.
    Finished,
    /// Held by the player.
    Paused,
}

/// Something that happened this step, for presentation to react to.
///
/// Events are the *only* channel from the simulation to the audio, the HUD
/// notifications and the particle effects. Presentation never inspects the sim
/// for "did something just happen" — it drains this list, which means a paused
/// or replayed frame produces exactly the effects it should.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RaceEvent {
    /// A number ticked over on the countdown (`3`, `2`, `1`).
    CountdownTick(u32),
    /// The countdown finished.
    Go,
    /// Something was hit.
    ///
    /// `severity` is what presentation keys off — sound, camera impulse, sparks
    /// — and `strength` only scales *within* that severity's band. `fresh`
    /// distinguishes the opening response of a collision from the rate-limited
    /// cue a sustained grind emits, which is what stops one long scrape from
    /// sounding like a dozen separate crashes.
    Impact {
        severity: Severity,
        strength: f32,
        traffic: bool,
        fresh: bool,
    },
    /// A traffic car was threaded.
    NearMiss { boost_awarded: f32 },
    /// A boosting player went through the back of a traffic car and put it off
    /// the road.
    SmashedThrough { boost_awarded: f32 },
    /// A boost pickup was collected.
    ///
    /// Carries the tier as well as the amount, because the cue is pitched by
    /// tier: a player who hears the difference between a green and a red without
    /// looking at the bar has learned the ladder, and that is the whole reason
    /// the tiers are named rather than numeric.
    PickupCollected { tier: BoostTier, boost_awarded: f32 },
    /// A drift began.
    DriftStarted,
    /// Boost was engaged.
    BoostStarted,
    /// The car left the tarmac.
    WentOffRoad,
    /// The car was returned to the last safe point.
    Reset,
    /// The finish line was crossed, after this many steps.
    Finished { steps: u64 },
}

/// The deterministic race.
#[derive(Debug, Clone)]
pub struct RaceSim {
    /// The compiled course. Immutable and shared: a `RaceSim` clone (the ghost,
    /// a replay, a test fixture) costs a refcount rather than a copy of nine
    /// kilometres of samples, and no clone can diverge from another's road.
    plan: Arc<CoursePlan>,
    car: CarState,
    /// The lane the phone game is driving toward, or `None` in the wheel game.
    /// This one `Option` is the whole of "the simulation is on rails" — see
    /// [`crate::PlayProfile`] for why the decision is made once, far above here.
    rails: Option<rails::RailsState>,
    /// Live collision state: which contact episodes are running, and whether the
    /// car is under recovery assistance. See [`contact`].
    contact: ContactState,
    traffic: Traffic,
    /// The course's boost pickups, and which of them this run has taken.
    pickups: PickupField,
    boost: BoostMeter,
    camera: ChaseCamera,
    tuning: Tuning,
    phase: RacePhase,
    step_n: u64,
    countdown_left: u32,
    countdown_number: u32,
    finish_step: u64,
    near_miss_notice: u32,
    go_banner: u32,
    near_miss_count: u32,
    impact_count: u32,
    top_speed_seen: f32,
    events: Vec<RaceEvent>,
    previous_car_pose: CarPose,
    car_pose: CarPose,
    /// Each traffic pool slot's `(distance, lateral)` as of the *previous* fixed
    /// step, or `None` where the slot was not in play.
    ///
    /// The player's car, the camera and the ghost have all interpolated between
    /// fixed steps since they existed; traffic did not, and at 60 Hz on a 60 Hz
    /// display nobody could see the difference because there was none to see. It
    /// becomes visible the moment the display refreshes faster than the
    /// simulation ticks: every other frame would repeat a traffic car's position
    /// while the player's own car moved, so the cars ahead judder at 60 Hz
    /// against a smooth 120 Hz world. See [`RaceSim::traffic_pose`].
    previous_traffic: Vec<Option<(f32, f32)>>,
    previous_camera_pose: CameraPose,
    camera_pose: CameraPose,
    last_forward_accel: f32,
    /// The camera kick owed to a collision resolved this step, consumed by
    /// [`RaceSim::repose`]. At most one per step, and only ever from a *fresh*
    /// contact — which is the mechanism behind "camera impulses are triggered
    /// once per impact episode".
    pending_impulse: Option<ImpactImpulse>,
    was_off_road: bool,
    was_boosting: bool,
}

impl RaceSim {
    /// Build the race for `seed` under `tuning`, at the start line, counting in.
    ///
    /// The wheel game. [`RaceSim::with_profile`] builds either.
    pub fn new(seed: u64, tuning: Tuning) -> RaceSim {
        RaceSim::with_profile(seed, tuning, crate::PlayProfile::Wheel)
    }

    /// Build the race for `seed` under `tuning` for `profile`.
    ///
    /// On [`crate::PlayProfile::Rails`] the car starts in the middle lane —
    /// the phone game opens with a choice in both directions rather than
    /// against a barrier.
    pub fn with_profile(seed: u64, tuning: Tuning, profile: crate::PlayProfile) -> RaceSim {
        // The shipping course for this seed, compiled through the ordinary
        // pipeline. Compilation happens exactly here, once per race — never on
        // a frame path.
        let plan = crate::course::procedural::plan_for(seed, &tuning)
            .unwrap_or_else(|error| panic!("the shipping course must compile: {error}"));
        RaceSim::from_plan(Arc::new(plan), tuning, profile)
    }

    /// Build the race on an already-compiled course.
    ///
    /// This is the door every other constructor goes through, and the one a
    /// hand-authored course, a validation harness or a replay uses: the plan is
    /// a value, so "which course is this" is answered by what you were handed
    /// rather than by re-running a generator and hoping it agrees.
    pub fn from_plan(
        plan: Arc<CoursePlan>,
        tuning: Tuning,
        profile: crate::PlayProfile,
    ) -> RaceSim {
        // Borrowed, not cloned. `RaceSim` has no `track` field — it reads
        // `self.plan.track()` — so this local existed only to be handed to the
        // four calls below, and cloning it copied the whole ~4 600-entry sample
        // table (~371 KB) to be dropped a few lines later. The borrow ends
        // before `plan` is moved into the struct.
        let track = plan.track();
        let mut car = CarState::parked(Vec3::ZERO, 0.0);
        controller::place_on_track(&mut car, &track.sample_at(GRID_DISTANCE), 0.0);
        let mut camera = ChaseCamera::new();
        camera.snap_to(&car, &track, &tuning.camera);
        let camera_pose = camera.step(
            &car,
            &track,
            &tuning.camera,
            &tuning.vehicle,
            crate::camera::CameraDrive {
                lane_locked: profile.is_rails(),
                ..crate::camera::CameraDrive::default()
            },
        );
        let car_pose = pose_of(&car, &track, 0.0);
        // Lane 0 is the centreline lane, and it exists for the whole course, so
        // "start in the middle" needs no lookup and can never be a lane that
        // stops existing further on.
        let rails = profile.is_rails().then(|| rails::RailsState::in_lane(0));
        RaceSim {
            rails,
            contact: ContactState::new(),
            traffic: Traffic::new(plan.clone(), &tuning.race),
            pickups: PickupField::new(plan.clone()),
            boost: BoostMeter::new(),
            camera,
            phase: RacePhase::Countdown,
            step_n: 0,
            countdown_left: tuning.race.countdown_steps * COUNTDOWN_NUMBERS,
            countdown_number: COUNTDOWN_NUMBERS,
            finish_step: 0,
            near_miss_notice: 0,
            go_banner: 0,
            near_miss_count: 0,
            impact_count: 0,
            top_speed_seen: 0.0,
            events: Vec::new(),
            previous_traffic: Vec::new(),
            previous_car_pose: car_pose,
            car_pose,
            previous_camera_pose: camera_pose,
            camera_pose,
            last_forward_accel: 0.0,
            pending_impulse: None,
            was_off_road: false,
            was_boosting: false,
            plan,
            car,
            tuning,
        }
    }

    /// Build the shipping race: the default seed and the default tuning, on the
    /// **wheel** game.
    ///
    /// The profile is worth spelling out now that the browser ships
    /// [`crate::PlayProfile::Rails`] on every device: this is the shipping
    /// *course*, not the shipping *control scheme*, and it stays on the wheel
    /// game because that is the car most of these tests are about. A rails race
    /// is [`RaceSim::with_profile`].
    pub fn shipping() -> RaceSim {
        RaceSim::new(crate::DEFAULT_SEED, Tuning::DEFAULT)
    }

    /// The course's road.
    pub fn track(&self) -> &Track {
        self.plan.track()
    }

    /// The compiled course — sections, traffic plans, encounters, opportunity
    /// windows and the validation report.
    pub fn plan(&self) -> &Arc<CoursePlan> {
        &self.plan
    }

    /// The car.
    pub const fn car(&self) -> &CarState {
        &self.car
    }

    /// The traffic pool.
    pub const fn traffic(&self) -> &Traffic {
        &self.traffic
    }

    /// The course's boost pickups and what this run has taken of them.
    pub const fn pickups(&self) -> &PickupField {
        &self.pickups
    }

    /// The boost meter.
    /// Whether this race is on rails — the phone game, where lateral position is
    /// chosen a lane at a time rather than steered.
    ///
    /// A driver has to know which of the two control schemes it is holding: the
    /// wheel game's lateral position is emergent from steering, the rails game's
    /// is commanded in lane hops, and a plan expressed in the wrong one does
    /// nothing at all. This is the same fact `step` already branches on
    /// internally, published rather than re-derived.
    pub const fn on_rails(&self) -> bool {
        self.rails.is_some()
    }

    /// The lane the rails car is committed to, or `None` off rails.
    ///
    /// Distinct from "the lane the car is nearest to": during a lane change the
    /// car is between two lanes but has already *chosen* one, and a driver that
    /// cannot tell the difference cannot tell an in-progress move from a
    /// finished one. Published because `DriveCommand::lane_step` is a
    /// **relative** control — it retargets from this value on every step it is
    /// non-zero — so a driver that cannot read it can only steer blind and hope,
    /// which is exactly what it had to do before this existed.
    pub fn rails_lane(&self) -> Option<i32> {
        self.rails.map(rails::RailsState::lane)
    }

    pub const fn boost(&self) -> &BoostMeter {
        &self.boost
    }

    /// The live collision state — which contacts are still in progress, and
    /// whether the car is recovering from one.
    pub const fn contact(&self) -> &ContactState {
        &self.contact
    }

    /// The tuning this race is running under.
    pub const fn tuning(&self) -> &Tuning {
        &self.tuning
    }

    /// The run phase.
    pub const fn phase(&self) -> RacePhase {
        self.phase
    }

    /// How many fixed steps have been taken.
    pub const fn step_count(&self) -> u64 {
        self.step_n
    }

    /// The countdown number currently showing (`0` once racing).
    pub const fn countdown_number(&self) -> u32 {
        self.countdown_number
    }

    /// Steps remaining on the near-miss notification (`0` = hidden).
    pub const fn near_miss_notice(&self) -> u32 {
        self.near_miss_notice
    }

    /// Steps remaining on the "GO" banner (`0` = hidden).
    pub const fn go_banner(&self) -> u32 {
        self.go_banner
    }

    /// Total near misses this run.
    pub const fn near_miss_count(&self) -> u32 {
        self.near_miss_count
    }

    /// Total impacts this run.
    pub const fn impact_count(&self) -> u32 {
        self.impact_count
    }

    /// The highest ground speed reached this run (m/s).
    pub const fn top_speed_seen(&self) -> f32 {
        self.top_speed_seen
    }

    /// Progress along the course, `0..1`.
    pub fn progress(&self) -> f32 {
        self.plan.track().progress(self.car.distance)
    }

    /// The section the car is currently in.
    pub fn section(&self) -> SectionKind {
        self.plan.track().sample_at(self.car.distance).section
    }

    /// Elapsed race time in seconds — a **step count**, not a clock reading.
    pub fn elapsed_seconds(&self) -> f32 {
        let steps = match self.phase {
            RacePhase::Finished => self.finish_step,
            _ => self.step_n,
        };
        steps as f32 * DT
    }

    /// Drain this step's events.
    pub fn take_events(&mut self) -> Vec<RaceEvent> {
        std::mem::take(&mut self.events)
    }

    /// This step's events without consuming them.
    pub fn events(&self) -> &[RaceEvent] {
        &self.events
    }

    /// The car pose for a render frame `alpha` of the way through the current
    /// step (`0` = the previous step, `1` = this one).
    pub fn car_pose(&self, alpha: f32) -> CarPose {
        CarPose::lerp(self.previous_car_pose, self.car_pose, alpha)
    }

    /// The camera pose for a render frame at `alpha`.
    pub fn camera_pose(&self, alpha: f32) -> CameraPose {
        CameraPose::lerp(self.previous_camera_pose, self.camera_pose, alpha)
    }

    /// Traffic slot `index`'s `(distance, lateral)` for a render frame `alpha` of
    /// the way through the current step, or `None` when the slot is not in play.
    ///
    /// Interpolated on the same terms as the player's car, so a display refreshing
    /// faster than the simulation ticks shows traffic moving as smoothly as
    /// everything else.
    ///
    /// A slot that was **not** active last step reports its current position
    /// un-interpolated: a car that has just entered play has no previous position
    /// to come from, and lerping it out of a stale one would fling it across the
    /// road on its first visible frame. The same guard covers a slot recycled onto
    /// a new plan, which is the other way a pool entry's distance can jump.
    pub fn traffic_pose(&self, index: usize, alpha: f32) -> Option<(f32, f32)> {
        let car = self.traffic.cars().get(index).filter(|c| c.active)?;
        let now = (car.distance, car.lateral);
        let was = self
            .previous_traffic
            .get(index)
            .copied()
            .flatten()
            .filter(|(d, _)| (now.0 - d).abs() < TRAFFIC_TELEPORT_METRES);
        Some(was.map_or(now, |(d, l)| {
            (d + (now.0 - d) * alpha, l + (now.1 - l) * alpha)
        }))
    }

    /// Traffic slot `index`'s wreck arc — its height above the road (m) and how
    /// far it has tumbled (rad) — or `None` when the car is not a wreck.
    ///
    /// Separate from [`Self::traffic_pose`] rather than folded into it because
    /// it answers a different question. `traffic_pose` is where a car *is*, and
    /// every car has an answer; this is the arc a wreck is on, and almost no car
    /// ever has one. Returning `None` is what lets the renderer keep posing the
    /// other fifteen cars through the cheap path.
    pub fn traffic_wreck(&self, index: usize, alpha: f32) -> Option<(f32, f32)> {
        self.traffic
            .cars()
            .get(index)
            .filter(|c| c.active & c.is_wreck())
            .map(|c| c.wreck_arc(alpha))
    }

    /// Record where every traffic car is, before the step that moves them.
    fn capture_traffic(&mut self) {
        let cars = self.traffic.cars();
        self.previous_traffic.clear();
        self.previous_traffic.extend(
            cars.iter()
                .map(|c| c.active.then_some((c.distance, c.lateral))),
        );
    }

    /// Advance one fixed step.
    pub fn step(&mut self, command: DriveCommand) {
        self.events.clear();
        let command = command.sanitised();
        self.previous_car_pose = self.car_pose;
        self.previous_camera_pose = self.camera_pose;
        self.capture_traffic();

        if command.restart {
            self.restart();
            return;
        }
        if command.pause {
            self.toggle_pause();
        }
        if self.phase == RacePhase::Paused {
            // A paused frame still re-poses, so the frozen frame renders, but
            // advances nothing.
            return;
        }
        if command.reset {
            self.reset_to_safe_point();
        }

        let effective = self.phase_command(command);
        self.drive(effective);
        self.advance_phase();
        self.step_n += 1;
        self.repose();
    }

    /// Restart the whole run from the start line, **in the same game**.
    ///
    /// The profile has to be carried across: `RaceSim::new` is the wheel game,
    /// so rebuilding through it silently turned a phone's lane game into the
    /// driving game on every restart. The profile is a property of the *device*,
    /// not of the run, and a restart does not change what device you are on.
    pub fn restart(&mut self) {
        let tuning = self.tuning;
        let profile = self
            .rails
            .is_some()
            .then_some(crate::PlayProfile::Rails)
            .unwrap_or(crate::PlayProfile::Wheel);
        // The **same compiled plan**, not a recompiled one. A restart must
        // reproduce the road and the traffic exactly, and re-running the
        // compiler to get back to where you already were is both slower and one
        // more place the two could disagree.
        *self = RaceSim::from_plan(self.plan.clone(), tuning, profile);
    }

    /// Place the car on the road centre at `distance` metres along, at rest,
    /// and snap the camera behind it.
    ///
    /// This is how the capture harness frames a specific section of the course
    /// without driving nine kilometres to reach it, and how a test targets a
    /// section directly. It is a legitimate simulation capability — "put the car
    /// here" — not a back door: it goes through the same placement the start
    /// line and the reset use, so the resulting state is always a valid one.
    pub fn place_at(&mut self, distance: f32) {
        let sample = self.plan.track().sample_at(distance);
        controller::place_on_track(&mut self.car, &sample, 0.0);
        self.camera.snap_to(&self.car, self.plan.track(), &self.tuning.camera);
        self.traffic.clear();
        // "The car I am still touching" is meaningless after a teleport, and the
        // traffic pool it referred to has just been emptied.
        self.contact.clear();
        self.repose();
    }

    /// Give the car a speed along its current heading — the other half of
    /// framing a capture, since a chase camera at a standstill shows none of
    /// what the camera actually does at speed.
    pub fn launch_at(&mut self, speed: f32) {
        self.car.forward_speed = speed.max(0.0);
        self.repose();
    }

    /// Return the car to the most recent safe point on the road.
    pub fn reset_to_safe_point(&mut self) {
        let sample = self.plan.track().safe_reset(self.car.distance);
        controller::place_on_track(&mut self.car, &sample, 0.0);
        self.camera.snap_to(&self.car, self.plan.track(), &self.tuning.camera);
        self.contact.clear();
        self.events.push(RaceEvent::Reset);
    }

    /// Hold or release the run.
    pub fn toggle_pause(&mut self) {
        self.phase = match self.phase {
            RacePhase::Paused => RacePhase::Racing,
            RacePhase::Racing | RacePhase::Countdown => RacePhase::Paused,
            RacePhase::Finished => RacePhase::Finished,
        };
    }

    /// The command the car actually receives, given the profile and the phase.
    ///
    /// The countdown and the finish take the wheel; everything else passes
    /// through — after the lane game has held the throttle down for the player.
    fn phase_command(&self, command: DriveCommand) -> DriveCommand {
        let command = self.held_throttle(command);
        match self.phase {
            // Held on the line. The hold is NOT expressed as a brake — see
            // `drive`, which holds the car explicitly.
            RacePhase::Countdown => DriveCommand {
                throttle: 0.0,
                brake: 0.0,
                boost: false,
                ..command
            },
            // Over the line: the car brakes itself to a stop, and then stops.
            // The brake is only applied while it is genuinely still rolling
            // forward, because a brake held past a standstill is reverse — the
            // finished car would otherwise drive itself back down the course.
            RacePhase::Finished => DriveCommand {
                throttle: 0.0,
                brake: (self.car.forward_speed > FINISH_ROLL_SPEED)
                    .then_some(0.8)
                    .unwrap_or(0.0),
                boost: false,
                steer: 0.0,
                handbrake: false,
                ..command
            },
            _ => command,
        }
    }

    /// **The lane game holds its own throttle.**
    ///
    /// A racing game asks the player for two things: how fast, and which line.
    /// The lane game already answered the first one — there is no cornering
    /// speed to judge, because the car cannot leave its lane by going too fast,
    /// and the only reason to lift is a car ahead, which is what changing lane
    /// is *for*. Leaving the throttle to the player therefore asks them to hold
    /// a button down for nine kilometres and punishes exactly one thing: letting
    /// go by accident.
    ///
    /// So it is held here rather than by a GAS button, and the button is gone
    /// (`touch::PadLayout::rails_for_viewport`). This is the same reasoning that
    /// removed the lane buttons — a control whose right answer never changes is
    /// not a decision, and the pad should only carry decisions.
    ///
    /// Applied **before** the phase rewrite above, so the countdown still holds
    /// the car on the line and the finish still brings it to a stop: those are
    /// the two moments the game genuinely does take the throttle away, and they
    /// keep the last word.
    ///
    /// The wheel game is untouched. There, throttle control *is* the game.
    fn held_throttle(&self, command: DriveCommand) -> DriveCommand {
        let held = f32::from(self.rails.is_some());
        DriveCommand {
            throttle: command.throttle.max(held),
            ..command
        }
    }

    /// Boost, car, traffic, contacts, near misses.
    fn drive(&mut self, command: DriveCommand) {
        // The countdown holds the car outright. Doing this with the brake — the
        // obvious way — reverses it off the line at nine metres a second,
        // because a stationary car being braked is a car being asked to reverse.
        if self.phase == RacePhase::Countdown {
            controller::settle_steering(&mut self.car, command, &self.tuning.vehicle);
            self.last_forward_accel = 0.0;
            // A held car swept nothing: it is where it was.
            self.resolve_traffic(self.car.distance);
            self.contact.advance(&mut self.car, &self.tuning.collision);
            return;
        }
        let boost_available = self.boost.step(command.boost, &self.car, &self.tuning.race);
        if self.boost.active() && !self.was_boosting {
            self.events.push(RaceEvent::BoostStarted);
        }
        self.was_boosting = self.boost.active();

        // Where the car was before it moved. Two things downstream need the
        // interval the car travelled rather than the point it ended at: the
        // pickup sweep (see [`pickups`]) and the near-miss test (see
        // [`collision::is_near_miss`]). Both are questions about what the car
        // went *past*, and a boosting car can go past something entirely
        // between two samples.
        let was_at = self.car.distance;

        let report = controller::step(
            &mut self.car,
            command,
            self.plan.track(),
            &self.tuning,
            boost_available,
            &mut self.contact,
            self.rails.as_mut(),
        );
        self.last_forward_accel = report.forward_accel;
        if report.drift_started {
            self.events.push(RaceEvent::DriftStarted);
        }
        if let Some(impact) = report.barrier_impact {
            self.report_impact(impact);
        }

        self.collect_pickups(was_at);
        self.resolve_traffic(was_at);
        self.note_surface();

        // One call, at the very end of the step, ages every episode and fades
        // the recovery. Doing it here rather than inside the controller is what
        // makes "one fixed step is one tick of every contact" true even though
        // barriers resolve in the controller and traffic resolves after it.
        self.contact.advance(&mut self.car, &self.tuning.collision);

        self.top_speed_seen = self.top_speed_seen.max(self.car.speed());
        self.near_miss_notice = self.near_miss_notice.saturating_sub(1);
        self.go_banner = self.go_banner.saturating_sub(1);
    }

    /// Award every pickup the car drove over between `was_at` and here.
    ///
    /// Placed **before** traffic resolution and deliberately independent of it.
    /// A near miss and a collision are two readings of one event and have to be
    /// ordered against each other; a pickup is neither. Driving over one is a
    /// fact about where the car went, and a car you clipped in the same step
    /// does not un-drive it.
    fn collect_pickups(&mut self, was_at: f32) {
        let race = self.tuning.race;
        let vehicle = self.tuning.vehicle;
        let taken = self.pickups.collect(
            was_at,
            &self.car,
            self.plan.track(),
            &race,
            &vehicle,
        );
        for collected in taken {
            self.boost.award(collected.boost);
            // The same notification a near miss raises: the reward is the bar
            // moving, and the HUD's "+" flash reads `BoostMeter::recent_gain`,
            // which `award` has just fed.
            self.near_miss_notice = race.notify_steps;
            self.events.push(RaceEvent::PickupCollected {
                tier: collected.tier,
                boost_awarded: collected.boost,
            });
        }
    }

    /// Traffic contacts first, then near misses on whatever was not hit.
    ///
    /// The order inside a contact matters and is fixed: **separate, then
    /// respond**. Separation runs on every overlapping step — it is the physics
    /// of two bodies not occupying one space, and suppressing it would leave the
    /// player interpenetrated. The *response* — momentum, sound, camera — is
    /// gated by the episode ledger, and runs once per collision.
    fn resolve_traffic(&mut self, was_at: f32) {
        self.traffic.step(
            self.car.distance,
            self.plan.track(),
            &self.tuning.race,
            &self.tuning.collision,
        );

        let race = self.tuning.race;
        let vehicle = self.tuning.vehicle;
        let collision = self.tuning.collision;
        let snapshot: Vec<(usize, f32, f32, f32, u32, bool, i32)> = self
            .traffic
            .cars()
            .iter()
            .enumerate()
            .filter(|(_, c)| c.active)
            .map(|(i, c)| (i, c.distance, c.lateral, c.speed, c.slot, c.near_missed, c.lane))
            .collect();

        // The player's lane, once per step rather than once per traffic car: it
        // is a property of where the car is, not of what it is passing.
        let player_lane = {
            let here = self.plan.track().sample_at(self.car.distance);
            self.plan.track().lane_at_lateral(&here, self.car.lateral)
        };

        for (index, distance, lateral, speed, slot, near_missed, lane) in snapshot {
            let obstacle = contact::Obstacle::Traffic { slot };
            let gap = collision::traffic_gap(&self.car, distance, lateral, &race, &vehicle);
            // Report the clearance every step, whether or not there is contact:
            // an episode ends the moment the pair genuinely comes apart, and
            // that fact is only visible from here.
            self.contact.note_gap(obstacle, gap, &collision);

            let Some(overlap) =
                collision::traffic_overlap(&self.car, distance, lateral, &race, &vehicle)
            else {
                if !near_missed
                    && collision::is_near_miss(
                        &self.car,
                        was_at,
                        player_lane,
                        distance,
                        lane,
                        speed,
                        &race,
                        &vehicle,
                    )
                {
                    self.traffic.mark_near_missed(index);
                    self.boost.award(race.near_miss_boost);
                    self.near_miss_count += 1;
                    self.near_miss_notice = race.notify_steps;
                    self.events.push(RaceEvent::NearMiss {
                        boost_awarded: race.near_miss_boost,
                    });
                }
                continue;
            };

            let sample = self.plan.track().sample_at(self.car.distance);
            // Which way round the obstacle has more room. A shunt has no natural
            // side, and biasing the player toward the middle of the road is what
            // turns "stuck behind a car" into "slide past it".
            let escape = escape_side(self.car.lateral, lateral, &sample);
            let facts =
                collision::traffic_facts(&self.car, &overlap, speed, slot, &sample, escape);

            // **Going through the back of it.** Handled before the contact
            // response and instead of it, not after: the point of a smash is
            // that none of the crash happens. No momentum exchange, no recovery
            // assist, no separation — the player's line is not disturbed at all,
            // because a boost that gets bounced off a hatchback is not a boost.
            //
            // The traffic car takes the entire event. It is wrecked, thrown to
            // whichever side has more room, and shoved forward by the hit.
            if collision::is_smash_through(&self.car, &facts) {
                self.smash_through(index, escape);
                continue;
            }

            let responded = self.contact.respond(&mut self.car, &facts, &collision);
            let length = self.plan.track().length();
            collision::separate_from_traffic(
                &mut self.car,
                &mut self.traffic.cars_mut()[index],
                &overlap,
                &sample,
                escape,
                length,
                &collision,
            );
            // A car you are touching is not a car you threaded, whether or not
            // the contact was loud enough to report.
            self.traffic.mark_near_missed(index);
            if let Some(impact) = responded {
                self.report_impact(impact);
            }
        }
    }

    /// Wreck the traffic car at `index`, pay the player for it, and kick the
    /// camera.
    ///
    /// # Why this pays less than a near miss
    ///
    /// [`RaceTuning::smash_boost`] is under half of
    /// [`RaceTuning::near_miss_boost`], and that ordering is the whole design of
    /// the mechanic rather than a number someone liked.
    ///
    /// A smash costs the player nothing — no speed, no line, no contact — so if
    /// it paid as well as threading did, the optimal race would be to hold boost
    /// and aim at the nearest bumper, and the game the boost economy exists to
    /// reward would be strictly worse than ignoring it. Paying *less* keeps the
    /// ordering the loop needs: threading traffic is the way to earn boost, and
    /// smashing through is what a boost you already have does to whatever is in
    /// the way. It is a spectacle with a tip attached, not an income.
    ///
    /// It is deliberately **not** an impact: it does not touch `impact_count`,
    /// does not open a contact episode, and raises no recovery assist. The only
    /// thing it borrows from a crash is the camera pulse, which is presentation
    /// and which a hit like this has plainly earned.
    fn smash_through(&mut self, index: usize, escape: f32) {
        let race = self.tuning.race;
        let shove = self.tuning.collision.traffic_yield_speed * SMASH_SHOVE_SHARE;
        // Thrown toward whichever side of the road has more room, the same
        // question a shunt already asks so a car is never punted into the
        // barrier the player is about to arrive at.
        self.traffic.wreck(index, escape, shove);

        self.boost.award(race.smash_boost);
        // The same "+" flash a near miss and a pickup raise: the reward is the
        // bar moving, and the HUD reads it from `BoostMeter::recent_gain`.
        self.near_miss_notice = race.notify_steps;
        self.events.push(RaceEvent::SmashedThrough {
            boost_awarded: race.smash_boost,
        });
        let direction = self.car.forward();
        let stronger = self
            .pending_impulse
            .is_none_or(|held| SMASH_CAMERA_PULSE > held.amplitude);
        if stronger {
            self.pending_impulse = Some(ImpactImpulse {
                direction,
                amplitude: SMASH_CAMERA_PULSE,
            });
        }
    }

    /// Push a resolved contact out as an event, and count it.
    ///
    /// Only a *fresh* contact counts as an impact. The rate-limited cues a
    /// sustained grind emits are presentation — a scrape you can hear and see —
    /// and counting them would turn one long rub against a wall into a HUD
    /// reading of forty crashes.
    fn report_impact(&mut self, impact: contact::Impact) {
        self.impact_count += u32::from(impact.fresh);
        // The strongest fresh contact of the step owns the camera kick. A
        // suppressed grind's cue carries a zero pulse and therefore never
        // re-arms it.
        let stronger = self
            .pending_impulse
            .is_none_or(|held| impact.pulse > held.amplitude);
        if impact.pulse > 0.0 && stronger {
            self.pending_impulse = Some(ImpactImpulse {
                direction: impact.direction,
                amplitude: impact.pulse,
            });
        }
        self.events.push(RaceEvent::Impact {
            severity: impact.severity,
            strength: impact.strength,
            traffic: impact.traffic,
            fresh: impact.fresh,
        });
    }

    /// Track the on/off-road transition so the HUD can warn once.
    fn note_surface(&mut self) {
        let off = self.car.surface.is_off_road();
        if off && !self.was_off_road {
            self.events.push(RaceEvent::WentOffRoad);
        }
        self.was_off_road = off;

        let stuck = off && self.car.speed() < self.tuning.race.stuck_speed;
        self.car.stuck_steps = if stuck {
            self.car.stuck_steps.saturating_add(1)
        } else {
            0
        };
    }

    /// Countdown ticks, and the finish line.
    fn advance_phase(&mut self) {
        match self.phase {
            RacePhase::Countdown => {
                self.countdown_left = self.countdown_left.saturating_sub(1);
                let per = self.tuning.race.countdown_steps.max(1);
                let showing = (self.countdown_left + per - 1) / per;
                if showing != self.countdown_number {
                    self.countdown_number = showing;
                    if showing > 0 {
                        self.events.push(RaceEvent::CountdownTick(showing));
                    }
                }
                if self.countdown_left == 0 {
                    self.phase = RacePhase::Racing;
                    self.countdown_number = 0;
                    self.go_banner = crate::hud::GO_BANNER_STEPS;
                    self.events.push(RaceEvent::Go);
                }
            }
            RacePhase::Racing => {
                if self.car.distance >= self.plan.track().length() - FINISH_MARGIN {
                    self.phase = RacePhase::Finished;
                    self.finish_step = self.step_n;
                    self.events.push(RaceEvent::Finished { steps: self.step_n });
                }
            }
            RacePhase::Finished | RacePhase::Paused => {}
        }
    }

    /// Refresh the interpolatable poses from the settled state.
    fn repose(&mut self) {
        self.car_pose = pose_of(&self.car, self.plan.track(), self.last_forward_accel);
        self.camera_pose = self.camera.step(
            &self.car,
            self.plan.track(),
            &self.tuning.camera,
            &self.tuning.vehicle,
            crate::camera::CameraDrive {
                forward_accel: self.last_forward_accel,
                boosting: self.boost.active(),
                impact: self.pending_impulse.take(),
                lane_locked: self.rails.is_some(),
            },
        );
    }

    /// Whether the car has been stuck long enough to prompt a reset.
    pub fn is_stuck(&self) -> bool {
        self.car.stuck_steps as f32 * DT >= self.tuning.race.stuck_seconds
    }
}

/// Which way round an obstacle the player should be biased to slide.
///
/// A nose-to-tail shunt has no natural side, and the difference between "you
/// were pushed back" and "you were pushed *round*" is the difference between
/// being stuck behind a car and overtaking it. The rule: take the side the
/// player is already leaning toward, unless that side has run out of road, in
/// which case take the roomier one. Both branches are pure functions of the
/// geometry, so the choice replays exactly.
fn escape_side(player_lateral: f32, traffic_lateral: f32, sample: &crate::track::TrackSample) -> f32 {
    let room_right = sample.half_width - traffic_lateral;
    let room_left = sample.half_width + traffic_lateral;
    let roomier = if room_right >= room_left { 1.0 } else { -1.0 };
    let natural = if player_lateral >= traffic_lateral { 1.0 } else { -1.0 };
    let natural_room = if natural > 0.0 { room_right } else { room_left };
    if natural_room >= ESCAPE_MIN_ROOM {
        natural
    } else {
        roomier
    }
}

/// Road (m) that must remain beyond an obstacle for the player to be biased that
/// way round it. Roughly a car's width plus a margin.
const ESCAPE_MIN_ROOM: f32 = 2.6;

/// How many numbers the countdown shows.
pub const COUNTDOWN_NUMBERS: u32 = 3;

/// How close to the end of the course counts as the finish line (m).
pub const FINISH_MARGIN: f32 = 12.0;

/// How far a traffic car's arc distance may move in one fixed step and still be
/// treated as having *travelled* there rather than been teleported (m).
///
/// Traffic tops out well under 100 m/s, so a fixed step moves a car under 2 m.
/// Anything past this is a pool slot being recycled onto a different plan
/// somewhere else on the course, and interpolating across it would drag a car
/// through the intervening kilometre in a sixtieth of a second.
const TRAFFIC_TELEPORT_METRES: f32 = 25.0;

/// Forward speed (m/s) below which the finished car stops braking itself, so it
/// rolls to a halt rather than reversing back down the course.
pub const FINISH_ROLL_SPEED: f32 = 1.5;

/// Build the presentation pose for a car state.
///
/// This is where the "suspension" lives, and it is entirely a lie told in the
/// presentation layer: the body pitches under acceleration, rolls out of a turn,
/// and sits on the road's banking. None of it feeds back into the simulation, so
/// none of it can destabilise anything.
pub fn pose_of(car: &CarState, track: &Track, forward_accel: f32) -> CarPose {
    let sample = track.interpolated_at(car.distance);
    // Nose up under power, nose down under braking, plus the road's own grade.
    let squat = (-forward_accel * PITCH_PER_ACCEL).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    let pitch = -sample.grade.atan() + squat;
    // Lean out of the corner, plus the road's banking.
    //
    // The magnitude is the simulation's real load transfer rather than the yaw
    // rate alone, so the body leans by as much as the corner is actually costing
    // the tyres: raise the centre of gravity and the car visibly rolls further,
    // for the same reason it grips less. The yaw rate only supplies the
    // direction.
    let lean = (car.yaw_rate.signum() * car.load_transfer * ROLL_PER_TRANSFER)
        .clamp(-ROLL_LIMIT, ROLL_LIMIT);
    let roll = sample.bank + lean;
    CarPose {
        position: car.position,
        yaw: car.yaw,
        pitch,
        roll,
        wheel_spin: car.wheel_spin,
        steer_angle: car.steer * VISUAL_STEER_ANGLE,
    }
}

/// Radians of body pitch per m/s² of forward acceleration.
const PITCH_PER_ACCEL: f32 = 0.0022;
/// Hard limit on the accel/brake pitch (radians).
const PITCH_LIMIT: f32 = 0.075;
/// Radians of body roll at full lateral load transfer (inside wheels lifting).
const ROLL_PER_TRANSFER: f32 = 0.19;
/// Hard limit on the cornering roll (radians).
const ROLL_LIMIT: f32 = 0.11;
/// Front-wheel steering angle at full lock (radians).
const VISUAL_STEER_ANGLE: f32 = 0.52;

/// How much of a shunt's speed budget a smash puts into the car it went
/// through. Most of it: the hit is the player's whole closing speed arriving at
/// once, and a wreck that ambles away reads as a car changing lane.
const SMASH_SHOVE_SHARE: f32 = 0.8;

/// The camera pulse a smash delivers, `0..1`.
///
/// Firm rather than violent. The kick is the only feedback the *player's* car
/// gives — nothing else about their state changes — so it has to sell the hit;
/// but a full-strength shake is the vocabulary of a crash, and this is
/// emphatically not one.
const SMASH_CAMERA_PULSE: f32 = 0.45;

#[cfg(test)]
mod tests {
    use super::*;

    fn racing() -> RaceSim {
        let mut sim = RaceSim::shipping();
        // Step through the countdown.
        while sim.phase() == RacePhase::Countdown {
            sim.step(DriveCommand::IDLE);
        }
        sim
    }

    /// The lane game, past the countdown.
    fn railed() -> RaceSim {
        let mut sim =
            RaceSim::with_profile(crate::DEFAULT_SEED, Tuning::DEFAULT, crate::PlayProfile::Rails);
        while sim.phase() == RacePhase::Countdown {
            sim.step(DriveCommand::IDLE);
        }
        sim
    }

    /// Put a traffic car directly in front of the player, **just** touching it
    /// nose-to-tail.
    ///
    /// The depth matters and is the whole reason this is a helper. A contact is
    /// classified along whichever axis is least penetrated
    /// ([`collision::traffic_overlap`]), so a car planted deep inside the player
    /// is a car overlapping them more along the road than across it — which is
    /// a *side-swipe*, correctly, however much it looks like a rear-end when you
    /// write the fixture. The moment a real rear-end happens is the moment the
    /// bumpers first touch, and that is what this builds.
    fn car_planted_ahead(sim: &mut RaceSim) {
        let touching =
            sim.tuning().vehicle.half_length + sim.tuning().race.traffic_half_length - 0.2;
        let ahead = sim.car().distance + touching;
        let lateral = sim.car().lateral;
        let cars = sim.traffic.cars_mut();
        cars[0].active = true;
        cars[0].distance = ahead;
        cars[0].lateral = lateral;
        cars[0].lane = 0;
        cars[0].speed = 20.0;
        cars[0].near_missed = false;
        cars[0].wreck_steps = 0;
    }

    /// **Going through the back of a car under boost.** The player keeps
    /// everything — line, speed, momentum — and the car does not.
    #[test]
    fn smashing_through_a_car_wrecks_it_and_pays_without_costing_anything() {
        let mut sim = racing();
        sim.car.forward_speed = 80.0;
        sim.car.boosting = true;
        car_planted_ahead(&mut sim);

        let before_speed = sim.car().forward_speed;
        let before_lateral = sim.car().lateral;
        let before_impacts = sim.impact_count();
        let before_boost = sim.boost().charge();

        sim.drive(DriveCommand { boost: true, ..DriveCommand::FLAT_OUT });

        assert!(
            sim.events()
                .iter()
                .any(|e| matches!(e, RaceEvent::SmashedThrough { .. })),
            "the smash was not reported: {:?}",
            sim.events()
        );
        assert!(sim.traffic.cars()[0].is_wreck(), "the car was not wrecked");
        assert_eq!(
            sim.impact_count(),
            before_impacts,
            "a smash is not a crash and must not be counted as one"
        );
        assert!(
            sim.car().forward_speed >= before_speed,
            "the player was slowed by going through it: {before_speed} -> {}",
            sim.car().forward_speed
        );
        assert!(
            (sim.car().lateral - before_lateral).abs() < 0.05,
            "the player was pushed off their line: {before_lateral} -> {}",
            sim.car().lateral
        );
        assert!(
            sim.boost().charge() > before_boost - sim.tuning().race.boost_drain_rate * DT,
            "the smash paid nothing"
        );
    }

    /// The wreck genuinely leaves the road, which is the part the bounded yields
    /// exist to prevent for ordinary contact.
    #[test]
    fn a_wreck_leaves_the_road_and_then_leaves_the_world() {
        let mut sim = racing();
        sim.car.forward_speed = 80.0;
        sim.car.boosting = true;
        car_planted_ahead(&mut sim);
        sim.drive(DriveCommand { boost: true, ..DriveCommand::FLAT_OUT });
        assert!(sim.traffic.cars()[0].is_wreck());

        let start_lateral = sim.traffic.cars()[0].lateral;
        let mut widest = 0.0f32;
        let mut airborne = false;
        (0..200).for_each(|_| {
            sim.step(DriveCommand::FLAT_OUT);
            let car = sim.traffic.cars()[0];
            widest = widest.max((car.lateral - start_lateral).abs());
            airborne |= car.is_wreck() && car.wreck_arc(0.0).0 > 0.5;
        });
        assert!(
            widest > 6.0,
            "the wreck only moved {widest} m sideways — it is still on the road"
        );
        assert!(airborne, "and it never left the ground");
        assert!(
            !sim.traffic.cars()[0].active || !sim.traffic.cars()[0].is_wreck(),
            "the wreck is still in the world two seconds later"
        );
    }

    /// The rule is a *rear* hit under boost, and both halves matter. A
    /// side-swipe is still a crash however fast you were going, and a rear-end
    /// off boost is still a rear-end.
    #[test]
    fn a_smash_needs_both_the_boost_and_the_back_of_the_car() {
        let mut coasting = racing();
        coasting.car.forward_speed = 80.0;
        coasting.car.boosting = false;
        car_planted_ahead(&mut coasting);
        coasting.drive(DriveCommand::FLAT_OUT);
        assert!(
            !coasting.traffic.cars()[0].is_wreck(),
            "a rear-end without boost wrecked the car"
        );
        assert!(
            coasting
                .events()
                .iter()
                .all(|e| !matches!(e, RaceEvent::SmashedThrough { .. })),
            "and it must not have paid"
        );

        // Alongside rather than in front: the same speed, the same boost, and a
        // contact that is the player's mistake.
        let mut sideways = racing();
        sideways.car.forward_speed = 80.0;
        sideways.car.boosting = true;
        let index = 0;
        {
            // Level with the player and just touching across the road — the
            // mirror of the fixture above, and the axis that makes it a crash.
            let here = sideways.car().distance;
            let lateral = sideways.car().lateral;
            let touching = sideways.tuning.vehicle.half_width
                + sideways.tuning.race.traffic_half_width
                - 0.2;
            let cars = sideways.traffic.cars_mut();
            cars[index].active = true;
            cars[index].distance = here;
            cars[index].lateral = lateral + touching;
            cars[index].speed = 20.0;
            cars[index].near_missed = false;
        }
        sideways.drive(DriveCommand { boost: true, ..DriveCommand::FLAT_OUT });
        assert!(
            !sideways.traffic.cars()[index].is_wreck(),
            "a side-swipe under boost must still be a crash"
        );
    }

    /// **A lane change is not a drift**, and the difference is worth real money.
    ///
    /// The crossing runs the lateral channel far past `drift_threshold` for its
    /// few frames, so a detector that only measures sideways speed calls every
    /// dodge a drift — which raises the DRIFT banner on a car that cannot drift,
    /// and pays `drift_boost_rate` for tapping a lane button. Left alone, "tap
    /// left, tap right" is an infinite boost supply and the whole earn-it
    /// economy is optional.
    #[test]
    fn hopping_lanes_is_never_a_drift_and_never_pays_drift_boost() {
        let mut sim = railed();
        let hop = |step: i8| DriveCommand { lane_step: step, ..DriveCommand::FLAT_OUT };

        // Drive it into a wall of lane changes: left, right, left, right.
        let mut hops = 0;
        (0..600).for_each(|i| {
            let step = [0, 1, 0, -1][(i / 8) % 4] as i8;
            hops += i32::from(step != 0);
            sim.step(hop(step));
            assert!(
                !sim.car().drifting,
                "step {i}: a railed car reported a drift (lateral {} m/s)",
                sim.car().lateral_speed
            );
        });
        assert!(hops > 20, "the fixture really did keep changing lane: {hops}");
        assert!(
            !sim.events().iter().any(|e| matches!(e, RaceEvent::DriftStarted)),
            "and no drift was ever announced"
        );
    }

    /// A pickup with no other pickup near it, for the tests that want to observe
    /// exactly one collection.
    ///
    /// Picking "the first one past 400 m" instead is a trap the shipping course
    /// walks straight into: its pickups come in **rows**, so a test that drives
    /// sixty metres onto one collects three and reports a bug that is not there.
    /// The isolation radius is comfortably over any authored row's span.
    fn isolated_pickup(sim: &RaceSim) -> crate::course::pickups::BoostPickup {
        let all = sim.plan().pickups();
        *all.iter()
            .find(|p| {
                all.iter()
                    .all(|other| (other.id == p.id) | ((other.at_m - p.at_m).abs() > 250.0))
            })
            .expect("the shipping course has a pickup standing on its own")
    }

    /// Traffic interpolates between fixed steps like everything else the player
    /// can see. This is what makes a display faster than the 60 Hz simulation
    /// look smooth rather than showing traffic stepping against a fluid world.
    #[test]
    fn traffic_interpolates_between_fixed_steps() {
        let mut sim = racing();
        (0..120).for_each(|_| sim.step(DriveCommand::FLAT_OUT));
        let live = (0..sim.traffic().cars().len())
            .find(|i| sim.traffic().cars()[*i].active)
            .expect("the shipping course has traffic in play");

        // `alpha = 1` is the pose as of the step just taken; after one more step
        // that same pose is what `alpha = 0` interpolates *from*.
        let settled = sim.traffic_pose(live, 1.0).expect("an active slot has a pose");
        sim.step(DriveCommand::FLAT_OUT);
        let start = sim.traffic_pose(live, 0.0).expect("still in play");
        let half = sim.traffic_pose(live, 0.5).expect("still in play");
        let end = sim.traffic_pose(live, 1.0).expect("still in play");

        // `alpha = 0` is where it was, `alpha = 1` is where it now is, and the
        // midpoint is genuinely between them rather than snapped to either end.
        assert!(
            (start.0 - settled.0).abs() < 1.0e-3,
            "alpha 0 continues from the previous step's settled pose"
        );
        assert!(end.0 > start.0, "the car moved down the course");
        let midpoint = (start.0 + end.0) * 0.5;
        assert!(
            (half.0 - midpoint).abs() < 1.0e-3,
            "alpha 0.5 should be halfway: {} vs {midpoint}",
            half.0
        );
    }

    /// A slot that has just entered play has no previous position to come from,
    /// and a recycled slot's previous position is somewhere else entirely. Either
    /// one, interpolated, flings a car across the course on its first visible
    /// frame — so both report their current pose exactly.
    #[test]
    fn a_car_entering_play_is_not_smeared_out_of_a_stale_position() {
        let mut sim = racing();
        (0..120).for_each(|_| sim.step(DriveCommand::FLAT_OUT));
        let live = (0..sim.traffic().cars().len())
            .find(|i| sim.traffic().cars()[*i].active)
            .expect("traffic is in play");

        // Forge the teleport: the slot's remembered position is a kilometre back.
        let now = sim.traffic().cars()[live].distance;
        sim.previous_traffic[live] = Some((now - 1_000.0, 0.0));
        let posed = sim.traffic_pose(live, 0.5).expect("still in play");
        assert!(
            (posed.0 - now).abs() < 1.0e-3,
            "a jump past TRAFFIC_TELEPORT_METRES must not be interpolated"
        );
    }

    #[test]
    fn an_inactive_traffic_slot_has_no_pose_at_any_alpha() {
        let sim = RaceSim::shipping();
        let idle = (0..sim.traffic().cars().len())
            .find(|i| !sim.traffic().cars()[*i].active)
            .expect("the pool starts with spare slots");
        assert!(sim.traffic_pose(idle, 0.0).is_none());
        assert!(sim.traffic_pose(idle, 0.5).is_none());
        // ...and an index past the pool is not a panic.
        assert!(sim.traffic_pose(usize::MAX, 0.5).is_none());
    }

    #[test]
    fn a_new_race_starts_on_the_line_counting_in() {
        let sim = RaceSim::shipping();
        assert_eq!(sim.phase(), RacePhase::Countdown);
        assert_eq!(sim.countdown_number(), COUNTDOWN_NUMBERS);
        assert_eq!(sim.car().forward_speed, 0.0);
        assert!(sim.progress() < 0.01);
        assert_eq!(sim.section(), SectionKind::StartStraight);
        assert_eq!(sim.step_count(), 0);
    }

    /// The opening shot must have a floor. The chase camera looks at ground
    /// behind the car, and the course ribbon stops dead at distance zero, so a
    /// grid on the first metre of tarmac frames the car against a hole. The
    /// camera's whole foreground has to land on road that exists.
    #[test]
    fn the_grid_leaves_road_behind_the_car_for_the_camera_to_stand_on() {
        let sim = RaceSim::shipping();
        assert!(
            sim.car().distance >= GRID_DISTANCE - sim.track().spacing(),
            "the car starts on the grid, not on the first metre: {}",
            sim.car().distance
        );
        // A camera that looks forward and down can only ever see ground *ahead*
        // of its own eye, so "the eye is over road" is exactly the condition for
        // the whole foreground of the shot being road. Hold it for the entire
        // countdown, which is when the opening frame is taken.
        let mut sim = sim;
        for _ in 0..=(sim.tuning().race.countdown_steps * COUNTDOWN_NUMBERS) {
            let pose = sim.camera_pose(1.0);
            let behind = sim
                .car()
                .position
                .subtract(pose.eye)
                .dot(sim.car().forward());
            let camera_distance = sim.car().distance - behind;
            assert!(
                camera_distance > 0.0,
                "the eye is {camera_distance} m along a course that starts at 0"
            );
            sim.step(DriveCommand::IDLE);
        }
    }

    #[test]
    fn the_countdown_ticks_down_and_then_releases_the_car() {
        let mut sim = RaceSim::shipping();
        let mut ticks: Vec<u32> = Vec::new();
        let mut went = false;
        for _ in 0..(RaceSim::shipping().tuning().race.countdown_steps * COUNTDOWN_NUMBERS + 5) {
            sim.step(DriveCommand::FLAT_OUT);
            for event in sim.events() {
                match event {
                    RaceEvent::CountdownTick(n) => ticks.push(*n),
                    RaceEvent::Go => went = true,
                    _ => {}
                }
            }
        }
        assert!(went, "the countdown finishes");
        assert_eq!(sim.phase(), RacePhase::Racing);
        assert_eq!(ticks, vec![2, 1], "3 shows from the start, then 2 and 1 tick in");
    }

    /// The car does not move during the countdown — in **either** direction.
    ///
    /// The original of this test asserted `forward_speed < 0.5`, a signed
    /// comparison on a value that turns out to go negative: the car was
    /// reversing off the line at nine metres a second and `-9.0 < 0.5` passed
    /// happily. Magnitude, and the position, are what matter.
    #[test]
    fn the_car_is_held_during_the_countdown() {
        let mut sim = RaceSim::shipping();
        let start = sim.car().position;
        let mut worst = 0.0f32;
        while sim.phase() == RacePhase::Countdown {
            sim.step(DriveCommand { boost: true, ..DriveCommand::FLAT_OUT });
            worst = worst.max(sim.car().position.distance(start));
            assert!(
                sim.car().forward_speed.abs() < 0.01,
                "the car moved at {} m/s during the countdown",
                sim.car().forward_speed
            );
        }
        assert!(worst < 0.01, "and it never left the line: {worst} m");
        assert_eq!(sim.car().position, start);
    }

    /// A finished car rolls to a stop and stays there — it does not brake past
    /// zero and drive itself back down the course.
    #[test]
    fn a_finished_car_stops_rather_than_reversing() {
        let mut sim = racing();
        sim.place_at(sim.track().length());
        sim.launch_at(40.0);
        for _ in 0..600 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        assert_eq!(sim.phase(), RacePhase::Finished);
        assert!(
            sim.car().forward_speed >= -0.01,
            "the finished car is reversing at {} m/s",
            sim.car().forward_speed
        );
        assert!(sim.car().forward_speed < 5.0, "and it has come to rest");
    }

    #[test]
    fn driving_makes_progress_along_the_course() {
        let mut sim = racing();
        for _ in 0..600 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        assert!(sim.car().distance > 300.0, "ten seconds covers ground");
        assert!(sim.progress() > 0.03);
        assert!(sim.top_speed_seen() > 40.0);
    }

    /// The headline determinism guarantee.
    #[test]
    fn an_identical_command_sequence_replays_identically() {
        let script: Vec<DriveCommand> = (0..2_400)
            .map(|i| DriveCommand {
                throttle: if i % 200 < 160 { 1.0 } else { 0.0 },
                brake: if i % 200 >= 180 { 1.0 } else { 0.0 },
                steer: ((i as f32) * 0.017).sin(),
                handbrake: i % 400 > 370,
                boost: i % 300 < 60,
                ..DriveCommand::IDLE
            })
            .collect();
        let run = || {
            let mut sim = RaceSim::shipping();
            for command in &script {
                sim.step(*command);
            }
            (
                sim.car,
                sim.boost,
                sim.traffic.cars().to_vec(),
                sim.camera_pose,
                sim.near_miss_count,
                sim.impact_count,
            )
        };
        let a = run();
        let b = run();
        assert_eq!(a.0, b.0, "car state");
        assert_eq!(a.1, b.1, "boost meter");
        assert_eq!(a.2, b.2, "traffic");
        assert_eq!(a.3, b.3, "camera");
        assert_eq!(a.4, b.4, "near misses");
        assert_eq!(a.5, b.5, "impacts");
    }

    /// The regression: a restart on a phone stayed the phone's game.
    #[test]
    fn restarting_keeps_the_game_the_device_is_playing() {
        let mut rails =
            RaceSim::with_profile(crate::DEFAULT_SEED, Tuning::DEFAULT, crate::PlayProfile::Rails);
        assert!(rails.rails.is_some());
        rails.restart();
        assert!(rails.rails.is_some(), "a restart kept the lane game");

        let mut wheel = RaceSim::shipping();
        wheel.restart();
        assert!(wheel.rails.is_none(), "and the driving game stayed the driving game");
    }

    #[test]
    fn two_seeds_generate_two_different_races() {
        let a = RaceSim::new(1, Tuning::DEFAULT);
        let b = RaceSim::new(2, Tuning::DEFAULT);
        assert_ne!(a.track().samples(), b.track().samples());
    }

    #[test]
    fn pausing_freezes_the_run_and_resuming_continues_it() {
        let mut sim = racing();
        for _ in 0..240 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        let frozen = *sim.car();
        let steps = sim.step_count();
        sim.step(DriveCommand { pause: true, ..DriveCommand::FLAT_OUT });
        assert_eq!(sim.phase(), RacePhase::Paused);
        for _ in 0..120 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        assert_eq!(*sim.car(), frozen, "nothing moved while paused");
        assert_eq!(sim.step_count(), steps, "and no steps were counted");

        sim.step(DriveCommand { pause: true, ..DriveCommand::FLAT_OUT });
        assert_eq!(sim.phase(), RacePhase::Racing);
        sim.step(DriveCommand::FLAT_OUT);
        assert_ne!(*sim.car(), frozen, "and it resumes");
    }

    #[test]
    fn a_reset_returns_the_car_to_a_valid_point_on_the_road() {
        let mut sim = racing();
        for _ in 0..900 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        let before = sim.car().distance;
        // Shove the car well off the road.
        let sample = sim.track().sample_at(before);
        sim.car.position = sample.at_lateral(sample.half_width + 40.0);
        sim.step(DriveCommand { reset: true, ..DriveCommand::IDLE });

        assert!(sim.events().contains(&RaceEvent::Reset));
        let sample = sim.track().sample_at(sim.car().distance);
        assert!(
            sim.car().lateral.abs() < sample.half_width,
            "back on the tarmac: {}",
            sim.car().lateral
        );
        assert!(sim.car().distance <= before, "and behind where it went wrong");
        assert!(sim.car().distance > before - 200.0, "but not by much");
        assert!(sim.car().is_finite());
    }

    #[test]
    fn restarting_rebuilds_the_same_race_from_the_line() {
        let mut sim = racing();
        for _ in 0..600 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        let course = sim.track().samples().to_vec();
        sim.step(DriveCommand { restart: true, ..DriveCommand::IDLE });
        assert_eq!(sim.phase(), RacePhase::Countdown);
        assert_eq!(sim.step_count(), 0);
        assert_eq!(sim.car().forward_speed, 0.0);
        assert_eq!(sim.track().samples(), course.as_slice(), "the same course");
    }

    #[test]
    fn reaching_the_end_of_the_course_finishes_the_race() {
        let mut sim = racing();
        // Teleport to just before the line rather than driving 9 km.
        let end = sim.track().length() - FINISH_MARGIN - 4.0;
        let sample = sim.track().sample_at(end);
        controller::place_on_track(&mut sim.car, &sample, 0.0);
        for _ in 0..120 {
            sim.step(DriveCommand::FLAT_OUT);
            if sim.phase() == RacePhase::Finished {
                break;
            }
        }
        assert_eq!(sim.phase(), RacePhase::Finished);
        assert!(sim.events().iter().any(|e| matches!(e, RaceEvent::Finished { .. }))
            || sim.elapsed_seconds() > 0.0);

        // The car brakes itself to a stop and the time stops moving.
        let time = sim.elapsed_seconds();
        for _ in 0..300 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        assert_eq!(sim.elapsed_seconds(), time, "the clock stopped at the line");
        assert!(sim.car().forward_speed < 10.0, "and the car is stopping");
    }

    #[test]
    fn near_misses_are_earned_from_traffic_and_pay_boost() {
        let mut sim = racing();
        // Empty the meter first, so any charge at the end was genuinely earned.
        while sim.boost().charge() > 0.0 {
            sim.step(DriveCommand { boost: true, ..DriveCommand::IDLE });
        }
        let mut awarded = 0.0f32;
        for _ in 0..9_000 {
            let command = crate::script::autopilot(sim.car(), sim.track());
            sim.step(command);
            for event in sim.events() {
                if let RaceEvent::NearMiss { boost_awarded } = event {
                    awarded += boost_awarded;
                }
            }
        }
        assert!(
            sim.near_miss_count() > 0,
            "a clean run through the traffic yields near misses"
        );
        assert!(awarded > 0.0, "and each one paid boost: {awarded}");
        assert!(sim.car().is_finite());
    }

    #[test]
    fn a_near_miss_notification_shows_and_then_expires() {
        let mut sim = racing();
        let race = sim.tuning().race;
        sim.near_miss_notice = race.notify_steps;
        for _ in 0..race.notify_steps {
            assert!(sim.near_miss_notice() > 0);
            sim.step(DriveCommand::FLAT_OUT);
        }
        assert_eq!(sim.near_miss_notice(), 0);
    }

    #[test]
    fn hitting_traffic_reports_an_impact() {
        let mut sim = racing();
        // On the racing line, not flat-out-and-straight: an unsteered car on a
        // curving road ends up pinned against a barrier, which is a different
        // test entirely.
        crate::script::drive_autopilot(&mut sim, 900);
        // Line the player up directly behind a real traffic car. Planting one
        // would not work: the traffic step re-derives every car's lateral from
        // its lane before contacts are resolved, so a hand-placed lateral is
        // overwritten before the collision test ever sees it.
        let target = sim
            .traffic()
            .active()
            .filter(|c| c.distance > sim.car().distance + 6.0)
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
            .copied()
            .expect("there is traffic ahead");
        // Line the car up properly: on the road, in the traffic car's lane,
        // pointed down the road, at speed. Moving the position alone is not
        // enough — the car keeps whatever heading it had and slides straight
        // back out of the overlap inside a single step.
        let approach = sim.track().sample_at(target.distance - 3.0);
        controller::place_on_track(&mut sim.car, &approach, target.lateral);
        sim.car.forward_speed = 80.0;
        let lateral_before = sim.car().lateral_speed;
        let impacts_before = sim.impact_count();
        sim.step(DriveCommand::FLAT_OUT);
        assert!(
            sim.events()
                .iter()
                .any(|e| matches!(e, RaceEvent::Impact { traffic: true, .. })),
            "the contact was reported: {:?}",
            sim.events()
        );
        assert!(sim.impact_count() > impacts_before);
        assert_ne!(
            sim.car().lateral_speed,
            lateral_before,
            "and it shoved the car"
        );
        assert!(sim.car().is_finite());
    }

    // ---------------------------------------------------------------------
    // Traffic collisions, end to end.
    //
    // Everything below drives a *staged* contact through the whole pipeline —
    // controller, traffic step, contact episodes, separation, events, camera —
    // rather than calling the resolver directly. The unit-level claims live in
    // `contact` and `collision`; what these prove is that the pipeline actually
    // delivers them, which is a different question and the one that was wrong.
    // ---------------------------------------------------------------------

    /// A race with exactly one traffic car, so a scenario owns the only thing
    /// on the road and no unrelated car can wander into the measurement.
    fn solo_traffic() -> Tuning {
        Tuning {
            race: crate::tuning::RaceTuning {
                traffic_active: 1,
                ..crate::tuning::RaceTuning::DEFAULT
            },
            ..Tuning::DEFAULT
        }
    }

    /// Stage a contact and return the race one step *before* it happens.
    ///
    /// `along` is how far behind the traffic car the player starts and `across`
    /// how far to its side, both in metres; the two speeds are m/s. The traffic
    /// car's lateral is read *after* a settling step rather than assumed,
    /// because the traffic step re-derives it from the lane and the in-lane
    /// wander — a hand-placed lateral is overwritten before any contact test
    /// ever sees it.
    fn staged(along: f32, across: f32, player_speed: f32, traffic_speed: f32) -> RaceSim {
        let mut sim = RaceSim::new(crate::DEFAULT_SEED, solo_traffic());
        while sim.phase() == RacePhase::Countdown {
            sim.step(DriveCommand::IDLE);
        }
        sim.place_at(STAGE_DISTANCE);
        // Several idle steps, not one: the spawn budget is one slot per pool
        // entry per step, and a slot skipped for landing inside the player's
        // safety region spends a slot of that budget. With a pool of one, the
        // first usable slot therefore takes a few steps to arrive.
        let index = (0..8)
            .find_map(|_| {
                sim.step(DriveCommand::IDLE);
                sim.traffic.cars().iter().position(|c| c.active)
            })
            .expect("the solo traffic car spawned");
        {
            let car = &mut sim.traffic.cars_mut()[index];
            car.distance = STAGE_DISTANCE + 40.0;
            car.speed = traffic_speed;
            // The near-miss reward is not what any of these measure, and a
            // pending one would add boost events to the stream.
            car.near_missed = true;
        }
        sim.step(DriveCommand::IDLE);

        let target = sim.traffic.cars()[index];
        let sample = sim.track().sample_at(target.distance - along);
        controller::place_on_track(&mut sim.car, &sample, target.lateral + across);
        sim.car.forward_speed = player_speed;
        sim.contact.clear();
        sim
    }

    /// Where staged scenarios happen — well past the traffic clear-start, on
    /// ordinary road.
    const STAGE_DISTANCE: f32 = 1_200.0;

    /// Every fresh impact reported over `steps` of `command`.
    fn impacts(sim: &mut RaceSim, command: DriveCommand, steps: u32) -> Vec<(Severity, f32)> {
        (0..steps)
            .flat_map(|_| {
                sim.step(command);
                sim.events()
                    .iter()
                    .filter_map(|e| match e {
                        RaceEvent::Impact {
                            severity,
                            strength,
                            fresh: true,
                            ..
                        } => Some((*severity, *strength)),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn a_shallow_pass_down_the_side_of_traffic_is_a_scrape() {
        let t = solo_traffic();
        let overlap = t.vehicle.half_width + t.race.traffic_half_width - 0.12;
        let mut sim = staged(0.0, overlap, 60.0, 45.0);
        let before = sim.car().forward_speed;
        let hits = impacts(&mut sim, DriveCommand::FLAT_OUT, 4);
        assert_eq!(hits.first().map(|h| h.0), Some(Severity::Scrape), "{hits:?}");
        assert!(
            sim.car().forward_speed >= before * t.collision.scrape_speed_floor,
            "a scrape left {} of {before} m/s",
            sim.car().forward_speed
        );
    }

    #[test]
    fn rear_ending_traffic_at_an_ordinary_closing_speed_is_a_bump() {
        let t = solo_traffic();
        let mut sim = staged(5.2, 0.0, 50.0, 30.0);
        let before = sim.car().forward_speed;
        let hits = impacts(&mut sim, DriveCommand::FLAT_OUT, 4);
        assert_eq!(hits.first().map(|h| h.0), Some(Severity::Bump), "{hits:?}");
        assert!(
            sim.car().forward_speed >= before * t.collision.bump_speed_floor,
            "a bump left {} of {before} m/s",
            sim.car().forward_speed
        );
        assert!(sim.car().forward_speed < before, "but it cost something");
    }

    #[test]
    fn ploughing_into_much_slower_traffic_is_a_major_crash() {
        let t = solo_traffic();
        let mut sim = staged(5.2, 0.0, 90.0, 24.0);
        let before = sim.car().forward_speed;
        let hits = impacts(&mut sim, DriveCommand::FLAT_OUT, 4);
        assert_eq!(hits.first().map(|h| h.0), Some(Severity::MajorCrash), "{hits:?}");
        assert!(
            sim.car().forward_speed >= before * t.collision.crash_speed_floor,
            "a crash left {} of {before} m/s",
            sim.car().forward_speed
        );
        // And it is genuinely still driving, not spun to a halt.
        assert!(sim.car().forward_speed > 40.0);
        assert!(sim.car().is_finite());
    }

    /// **The headline regression.** Riding alongside a traffic car, leaning on
    /// it the whole time, must cost its momentum once.
    ///
    /// Before contact episodes this was the worst case in the game: the full
    /// response fired every fixed step for as long as the boxes overlapped, so
    /// two seconds of rubbing along a car took the player from racing speed to
    /// walking pace with a thud and a camera kick every step.
    ///
    /// The scenario is deliberately a *matched-speed coast* rather than a
    /// flat-out chase: at matched speed the pair genuinely stays abreast for the
    /// whole two seconds, which is the only way to make the overlap sustained
    /// rather than something the player drives out of in three steps. The
    /// steering leans into the traffic car throughout, so separation is fighting
    /// a live input the whole time.
    #[test]
    fn sustained_side_by_side_contact_costs_its_momentum_once() {
        let t = solo_traffic();
        let overlap = t.vehicle.half_width + t.race.traffic_half_width - 0.2;
        let lean = DriveCommand {
            steer: GRIND_STEER,
            ..DriveCommand::IDLE
        };

        // The control: the identical two seconds with nothing to touch. Coasting
        // costs speed on its own (drag and rolling resistance), so the question
        // is what the *contact* added on top of that.
        let coasted = {
            let mut sim = staged(0.0, overlap, GRIND_SPEED, GRIND_SPEED);
            sim.traffic.clear();
            (0..GRIND_STEPS).for_each(|_| sim.step(lean));
            sim.car().forward_speed
        };

        let mut sim = staged(0.0, overlap, GRIND_SPEED, GRIND_SPEED);
        let before = sim.car().forward_speed;
        let mut contact_steps = 0u32;
        let mut hits: Vec<(Severity, f32)> = Vec::new();
        for _ in 0..GRIND_STEPS {
            sim.step(lean);
            let touching = sim.events().iter().any(
                |e| matches!(e, RaceEvent::Impact { traffic: true, .. }),
            );
            contact_steps += u32::from(touching);
            hits.extend(sim.events().iter().filter_map(|e| match e {
                RaceEvent::Impact {
                    severity,
                    strength,
                    traffic: true,
                    fresh: true,
                } => Some((*severity, *strength)),
                _ => None,
            }));
            assert!(
                !sim.car().surface.is_off_road(),
                "the grind wandered off the road, so this stopped being a traffic test"
            );
        }

        assert!(contact_steps > 0, "the contact happened at all");
        // Every contact in a grind is a scrape. Nothing about rubbing along a
        // car escalates, however long you hold it there.
        assert!(
            hits.iter().all(|(s, _)| *s == Severity::Scrape),
            "rubbing along a car escalated: {hits:?}"
        );
        // And there are only as many full responses as there are cooldowns in
        // two seconds — four, not a hundred and twenty.
        let episodes = (GRIND_STEPS / t.collision.episode_steps + 1) as usize;
        assert!(
            hits.len() <= episodes,
            "{} full impacts in one grind; at most {episodes} cooldowns fit: {hits:?}",
            hits.len()
        );

        // The measurement that matters. Two seconds of continuous scraping cost
        // barely more than two seconds of coasting past.
        //
        // The comparison is deliberately one-sided. Contact changes the *line*
        // as well as the speed: the traffic car and the separation assist
        // together hold the player straighter than the same steering input does
        // on open road, and cornering costs speed of its own, so a grind can
        // legitimately finish marginally *faster* than the control. What must
        // never happen is the other direction.
        let grind_cost = coasted - sim.car().forward_speed;
        assert!(
            sim.car().forward_speed >= coasted * t.collision.scrape_speed_floor,
            "two seconds of scraping cost {grind_cost:.2} m/s on top of coasting              ({} vs {coasted} m/s), past the scrape floor",
            sim.car().forward_speed
        );
        assert!(
            sim.car().forward_speed > before * 0.9,
            "and the car is still going: {before} -> {} m/s",
            sim.car().forward_speed
        );
        assert!(sim.car().is_finite());
    }

    /// Matched speeds, so the pair genuinely stays abreast for the whole grind.
    const GRIND_SPEED: f32 = 55.0;
    /// Leaning into the traffic car — enough that separation is fighting a live
    /// input, not enough to steer off the road inside two seconds.
    const GRIND_STEER: f32 = 0.35;
    /// Two seconds of it.
    const GRIND_STEPS: u32 = 120;

    /// The same grind, seen from the audio and camera side: a contact episode
    /// schedules a bounded number of cues and arms exactly one camera impulse.
    #[test]
    fn a_grind_is_rate_limited_in_sound_and_kicks_the_camera_once_per_episode() {
        let t = solo_traffic();
        let overlap = t.vehicle.half_width + t.race.traffic_half_width - 0.2;
        let mut sim = staged(0.0, overlap, GRIND_SPEED, GRIND_SPEED);
        let lean = DriveCommand {
            steer: GRIND_STEER,
            ..DriveCommand::IDLE
        };
        let (mut cues, mut kicks) = (0u32, 0u32);
        for _ in 0..GRIND_STEPS {
            sim.step(lean);
            cues += sim
                .events()
                .iter()
                .filter(|e| matches!(e, RaceEvent::Impact { traffic: true, .. }))
                .count() as u32;
            kicks += u32::from(sim.events().iter().any(|e| {
                matches!(
                    e,
                    RaceEvent::Impact {
                        traffic: true,
                        fresh: true,
                        ..
                    }
                )
            }));
        }
        assert!(cues > 1, "a grind is audible and continuous: {cues} cues");
        // Rate limited to the scrape cadence, not one per step.
        let ceiling = GRIND_STEPS / t.collision.scrape_repeat_steps + 2;
        assert!(
            cues <= ceiling,
            "{cues} cues in {GRIND_STEPS} steps, ceiling {ceiling}"
        );
        // The camera is armed only by the opening response of an episode, so it
        // is strictly rarer than the sound.
        assert!(kicks < cues, "camera kicks are rarer than cues: {kicks} vs {cues}");
        assert!(
            kicks <= GRIND_STEPS / t.collision.episode_steps + 1,
            "{kicks} camera kicks in one grind"
        );
    }

    /// A cooldown must never make the player intangible: an unrelated car hit
    /// during one still lands.
    #[test]
    fn a_second_traffic_car_still_lands_during_the_first_ones_cooldown() {
        let mut sim = RaceSim::new(crate::DEFAULT_SEED, Tuning::DEFAULT);
        while sim.phase() == RacePhase::Countdown {
            sim.step(DriveCommand::IDLE);
        }
        crate::script::drive_autopilot(&mut sim, 900);
        // Two contacts against two different cars, back to back.
        let mut struck: Vec<u32> = Vec::new();
        for _ in 0..2 {
            let target = sim
                .traffic()
                .active()
                .filter(|c| c.distance > sim.car().distance + 6.0)
                .min_by(|a, b| a.distance.total_cmp(&b.distance))
                .copied()
                .expect("traffic ahead");
            let approach = sim.track().sample_at(target.distance - 3.0);
            controller::place_on_track(&mut sim.car, &approach, target.lateral);
            sim.car.forward_speed = 85.0;
            sim.step(DriveCommand::FLAT_OUT);
            let landed = sim
                .events()
                .iter()
                .any(|e| matches!(e, RaceEvent::Impact { fresh: true, traffic: true, .. }));
            assert!(landed, "the contact against slot {} landed", target.slot);
            struck.push(target.slot);
        }
        assert_ne!(struck[0], struck[1], "two genuinely different cars");
        assert!(sim.car().is_finite());
    }

    /// Every input keeps working through every severity. No stun, no lock, no
    /// frozen frames — the player is always the one responsible for the fix.
    #[test]
    fn the_player_keeps_every_control_through_every_severity() {
        let scenarios = [
            (Severity::Scrape, 0.0, 1.93f32, 60.0, 45.0),
            (Severity::Bump, 5.2, 0.0, 50.0, 30.0),
            (Severity::MajorCrash, 5.2, 0.0, 90.0, 24.0),
        ];
        for (expected, along, across, player, traffic) in scenarios {
            let mut sim = staged(along, across, player, traffic);
            let hits = impacts(&mut sim, DriveCommand::FLAT_OUT, 4);
            assert_eq!(hits.first().map(|h| h.0), Some(expected), "{hits:?}");

            // Throttle: the car accelerates on the very next step.
            let before = sim.car().forward_speed;
            sim.step(DriveCommand::FLAT_OUT);
            assert!(
                sim.car().forward_speed > before,
                "{expected:?}: throttle was dead after the hit"
            );

            // Steering: full lock actually turns the car.
            let yaw_before = sim.car().yaw;
            for _ in 0..10 {
                sim.step(DriveCommand::turning(1.0));
            }
            assert!(
                crate::track::shortest_angle(sim.car().yaw - yaw_before).abs() > 0.01,
                "{expected:?}: steering was dead after the hit"
            );

            // Braking, and the handbrake, both bite.
            let rolling = sim.car().forward_speed;
            for _ in 0..10 {
                sim.step(DriveCommand {
                    brake: 1.0,
                    ..DriveCommand::IDLE
                });
            }
            assert!(
                sim.car().forward_speed < rolling,
                "{expected:?}: the brake was dead after the hit"
            );
            for _ in 0..10 {
                sim.step(DriveCommand {
                    handbrake: true,
                    ..DriveCommand::turning(1.0)
                });
            }
            // Boost: engages when the meter allows, and is never blocked by a
            // collision.
            sim.boost.award(1.0);
            sim.step(DriveCommand {
                boost: true,
                ..DriveCommand::FLAT_OUT
            });
            assert!(
                sim.car().boosting,
                "{expected:?}: boost was refused after the hit"
            );
            assert!(sim.car().is_finite());
        }
    }

    /// A collision must not be a way to earn or spend boost, in either
    /// direction — the recovery assist is forgiving handling, not a power-up.
    #[test]
    fn a_collision_neither_awards_nor_consumes_boost() {
        // Measured against a control, because the meter is *always* moving: the
        // high-speed trickle pays out every step above the threshold, and this
        // scenario is deliberately run well above it. The claim is that the
        // collision changes nothing, not that nothing changes.
        let control = {
            let mut sim = staged(5.2, 0.0, 90.0, 24.0);
            let before = sim.boost().charge();
            sim.traffic.clear();
            sim.step(DriveCommand::FLAT_OUT);
            sim.boost().charge() - before
        };

        let mut sim = staged(5.2, 0.0, 90.0, 24.0);
        let charge = sim.boost().charge();
        let hits = impacts(&mut sim, DriveCommand::FLAT_OUT, 1);
        assert!(!hits.is_empty(), "the crash happened");
        assert!(
            (sim.boost().charge() - charge - control).abs() < 1.0e-6,
            "the collision moved the meter by {} beyond the ordinary trickle",
            sim.boost().charge() - charge - control
        );
        assert!(
            !sim.events()
                .iter()
                .any(|e| matches!(e, RaceEvent::BoostStarted | RaceEvent::NearMiss { .. })),
            "and it fired no boost events: {:?}",
            sim.events()
        );
        assert!(sim.contact().is_recovering(), "but recovery did start");
    }

    /// Recovery gets the player back to racing speed in about a second under
    /// throttle — the brief's one quantitative feel requirement.
    #[test]
    fn a_bump_at_full_throttle_is_recovered_from_in_about_a_second() {
        let mut sim = staged(5.2, 0.0, 50.0, 30.0);
        let before = sim.car().forward_speed;
        impacts(&mut sim, DriveCommand::FLAT_OUT, 1);
        let after = sim.car().forward_speed;
        assert!(after < before, "the bump cost speed: {before} -> {after}");

        // Sixty steps is one second.
        for _ in 0..60 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        assert!(
            sim.car().forward_speed >= before,
            "a second of throttle got back to {} of the {before} m/s lost",
            sim.car().forward_speed
        );
        assert!(!sim.contact().is_recovering(), "and the assist has finished");
    }

    /// The whole pipeline stays finite and deterministic through a scripted
    /// sequence of several genuine traffic contacts.
    #[test]
    fn a_scripted_sequence_of_traffic_contacts_replays_identically() {
        let run = || {
            let mut sim = RaceSim::new(crate::DEFAULT_SEED, Tuning::DEFAULT);
            while sim.phase() == RacePhase::Countdown {
                sim.step(DriveCommand::IDLE);
            }
            crate::script::drive_autopilot(&mut sim, 600);
            // Chase whatever is ahead and drive straight into it, repeatedly.
            let mut contacts = 0u32;
            for _ in 0..3_000 {
                let car = *sim.car();
                let command = sim
                    .traffic()
                    .active()
                    .filter(|c| c.distance > car.distance + 2.0)
                    .min_by(|a, b| a.distance.total_cmp(&b.distance))
                    .map(|t| DriveCommand {
                        throttle: 1.0,
                        steer: crate::script::steer_toward_line(&car, sim.track(), t.lateral),
                        ..DriveCommand::IDLE
                    })
                    .unwrap_or_else(|| crate::script::autopilot(&car, sim.track()));
                sim.step(command);
                contacts += u32::from(
                    sim.events()
                        .iter()
                        .any(|e| matches!(e, RaceEvent::Impact { fresh: true, .. })),
                );
                assert!(sim.car().is_finite(), "the run stayed finite");
            }
            (
                contacts,
                *sim.car(),
                sim.traffic().cars().to_vec(),
                sim.camera_pose,
                sim.impact_count(),
                sim.contact().clone(),
            )
        };
        let a = run();
        let b = run();
        assert!(a.0 >= 3, "the script genuinely hit things: {} contacts", a.0);
        assert_eq!(a.0, b.0, "contact count");
        assert_eq!(a.1, b.1, "car state");
        assert_eq!(a.2, b.2, "traffic, including its yields");
        assert_eq!(a.3, b.3, "camera");
        assert_eq!(a.4, b.4, "impact count");
        assert_eq!(a.5, b.5, "contact episodes and recovery");
    }

    /// A traffic car that is hit is nudged, and only nudged.
    #[test]
    fn contact_yields_the_traffic_car_within_its_bounds_and_it_returns() {
        let t = solo_traffic();
        let mut sim = staged(5.2, 0.0, 85.0, 26.0);
        let lane_before = sim.traffic().cars()[0].lane;
        impacts(&mut sim, DriveCommand::FLAT_OUT, 2);
        let hit = sim.traffic().cars()[0];
        assert!(
            hit.yield_speed != 0.0 || hit.yield_offset != 0.0,
            "the traffic car was moved by being hit"
        );
        assert!(hit.yield_offset.abs() <= t.collision.traffic_yield_lateral + 1.0e-4);
        assert!(hit.yield_speed.abs() <= t.collision.traffic_yield_speed + 1.0e-4);
        assert_eq!(hit.lane, lane_before, "and it never changes lane");

        for _ in 0..300 {
            sim.step(DriveCommand::IDLE);
        }
        let settled = sim.traffic().cars()[0];
        assert!(
            !settled.active || (settled.yield_offset == 0.0 && settled.yield_speed == 0.0),
            "and it returns to its lane: {settled:?}"
        );
    }

    #[test]
    fn going_off_road_is_reported_once_per_excursion() {
        let mut sim = racing();
        // On the racing line, not flat-out-and-straight. An unsteered car on a
        // curving road is already off the tarmac by the time this test starts
        // measuring, so the excursion it is trying to observe has already
        // happened and the "leaving the road" edge never fires.
        crate::script::drive_autopilot(&mut sim, 600);
        assert!(
            !sim.car().surface.is_off_road(),
            "the test starts on the tarmac"
        );
        let sample = sim.track().sample_at(sim.car().distance);
        sim.car.position = sample.at_lateral(sample.half_width + 3.0);
        sim.step(DriveCommand::FLAT_OUT);
        let first = sim.events().contains(&RaceEvent::WentOffRoad);
        sim.step(DriveCommand::FLAT_OUT);
        let second = sim.events().contains(&RaceEvent::WentOffRoad);
        assert!(first, "leaving the road is reported");
        assert!(!second, "and not repeated every step");
    }

    #[test]
    fn sitting_still_off_road_eventually_counts_as_stuck() {
        let mut sim = racing();
        let sample = sim.track().sample_at(200.0);
        sim.car.position = sample.at_lateral(sample.half_width + 6.0);
        sim.car.distance = 200.0;
        assert!(!sim.is_stuck());
        for _ in 0..600 {
            sim.step(DriveCommand::IDLE);
        }
        assert!(sim.is_stuck(), "the reset prompt appears");
        // Driving back on clears it.
        sim.reset_to_safe_point();
        sim.step(DriveCommand::FLAT_OUT);
        assert!(!sim.is_stuck());
    }

    #[test]
    fn boost_events_fire_on_engagement_only() {
        let mut sim = racing();
        for _ in 0..120 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        sim.step(DriveCommand { boost: true, ..DriveCommand::FLAT_OUT });
        assert!(sim.events().contains(&RaceEvent::BoostStarted));
        sim.step(DriveCommand { boost: true, ..DriveCommand::FLAT_OUT });
        assert!(
            !sim.events().contains(&RaceEvent::BoostStarted),
            "not every step it is held"
        );
    }

    /// **The feature, end to end.** Put the car on a pickup's lane, drive
    /// through it, and the meter goes up by exactly what the tier pays, once.
    #[test]
    fn driving_over_a_pickup_fills_the_meter_and_reports_it() {
        let mut sim = racing();
        let target = isolated_pickup(&sim);
        let expected = sim.tuning().race.pickup_boost(target.tier);

        // Line the car up on the pickup's lane, a little short of it.
        sim.place_at(target.at_m - 60.0);
        let sample = sim.track().sample_at(target.at_m);
        sim.car.lateral = sim.track().lane_lateral(&sample, target.lane);
        sim.launch_at(50.0);

        let before = sim.boost().charge();
        let mut collected = 0u32;
        for _ in 0..180 {
            // Coast: no throttle steering the car off its lane, no boost
            // draining the meter, so the only thing that can move the charge is
            // the pickup.
            sim.step(DriveCommand::default());
            collected += sim
                .events()
                .iter()
                .filter(|e| matches!(e, RaceEvent::PickupCollected { .. }))
                .count() as u32;
        }
        assert_eq!(collected, 1, "the pickup was not collected exactly once");
        assert_eq!(sim.pickups().collected(), 1);
        assert!(
            (sim.boost().charge() - before - expected).abs() < 1.0e-4,
            "the meter moved by {} rather than {expected}",
            sim.boost().charge() - before
        );
    }

    /// The event carries the tier, because the cue and the notification are
    /// pitched by it.
    #[test]
    fn the_collection_event_names_the_tier_it_paid() {
        let mut sim = racing();
        let target = isolated_pickup(&sim);
        sim.place_at(target.at_m - 40.0);
        let sample = sim.track().sample_at(target.at_m);
        sim.car.lateral = sim.track().lane_lateral(&sample, target.lane);
        sim.launch_at(50.0);

        let mut seen = None;
        for _ in 0..180 {
            sim.step(DriveCommand::default());
            seen = seen.or_else(|| {
                sim.events().iter().find_map(|e| match e {
                    RaceEvent::PickupCollected { tier, boost_awarded } => {
                        Some((*tier, *boost_awarded))
                    }
                    _ => None,
                })
            });
        }
        let (tier, awarded) = seen.expect("no collection event");
        assert_eq!(tier, target.tier);
        assert_eq!(awarded, sim.tuning().race.pickup_boost(target.tier));
    }

    /// A restart puts every pickup back. It is a new run, and the point of the
    /// ledger living on the *run* rather than on the plan is that the plan is
    /// shared and cannot be spent.
    #[test]
    fn a_restart_puts_every_pickup_back() {
        let mut sim = racing();
        let target = isolated_pickup(&sim);
        sim.place_at(target.at_m - 40.0);
        let sample = sim.track().sample_at(target.at_m);
        sim.car.lateral = sim.track().lane_lateral(&sample, target.lane);
        sim.launch_at(50.0);
        for _ in 0..180 {
            sim.step(DriveCommand::default());
        }
        assert_eq!(sim.pickups().collected(), 1);

        sim.step(DriveCommand { restart: true, ..DriveCommand::default() });
        assert_eq!(sim.pickups().collected(), 0);
        assert!(!sim.pickups().is_taken(&target));
        assert_eq!(sim.pickups().total(), sim.plan().pickups().len());
    }

    /// A pickup pays once per run, however many times the car crosses it. The
    /// ledger, not the sweep, is what guarantees that — the sweep has no memory.
    #[test]
    fn a_pickup_cannot_be_farmed_by_crossing_it_twice() {
        let mut sim = racing();
        let target = isolated_pickup(&sim);
        // One approach: onto the pickup's lane, short of it, and drive through.
        let approach = |sim: &mut RaceSim| {
            sim.place_at(target.at_m - 50.0);
            let sample = sim.track().sample_at(target.at_m);
            // `place_at` puts the car on the centreline, so the lane has to be
            // taken *after* it — a detail that silently made an earlier version
            // of this test pass for the wrong reason.
            sim.car.lateral = sim.track().lane_lateral(&sample, target.lane);
            sim.launch_at(40.0);
            (0..180).for_each(|_| sim.step(DriveCommand::default()));
        };

        approach(&mut sim);
        assert_eq!(sim.pickups().collected(), 1, "the first pass paid");
        assert!(sim.pickups().is_taken(&target));

        approach(&mut sim);
        assert_eq!(sim.pickups().collected(), 1, "a pickup paid twice");
    }

    /// Collection is part of the deterministic step, so the same commands
    /// produce the same collections — which is what a replay and the ghost both
    /// depend on.
    #[test]
    fn pickup_collection_replays_identically() {
        let run = || {
            let mut sim = racing();
            let mut taken = Vec::new();
            for _ in 0..900 {
                sim.step(DriveCommand::FLAT_OUT);
                taken.extend(sim.events().iter().filter_map(|e| match e {
                    RaceEvent::PickupCollected { tier, boost_awarded } => {
                        Some((*tier, *boost_awarded))
                    }
                    _ => None,
                }));
            }
            (taken, sim.pickups().collected(), sim.boost().charge())
        };
        let a = run();
        let b = run();
        assert_eq!(a, b);
        assert!(a.1 > 0, "the run crossed no pickups at all");
    }

    #[test]
    fn poses_interpolate_between_the_last_two_steps() {
        let mut sim = racing();
        for _ in 0..300 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        let start = sim.car_pose(0.0);
        let end = sim.car_pose(1.0);
        let mid = sim.car_pose(0.5);
        assert_ne!(start.position, end.position, "the car moved");
        assert!(mid.position.distance(start.position) < end.position.distance(start.position));

        let cam_start = sim.camera_pose(0.0);
        let cam_end = sim.camera_pose(1.0);
        assert_ne!(cam_start.eye, cam_end.eye);
        assert_eq!(sim.camera_pose(1.0), cam_end);
    }

    #[test]
    fn the_body_pose_pitches_under_power_and_rolls_in_a_turn() {
        let sim = racing();
        let track = sim.track();
        let mut car = *sim.car();
        car.forward_speed = 60.0;

        let accelerating = pose_of(&car, track, 30.0);
        let braking = pose_of(&car, track, -40.0);
        // Positive pitch is nose-DOWN (see `CarPose::pitch`), so power gives the
        // smaller value and braking the larger.
        assert!(accelerating.pitch < braking.pitch, "the nose lifts under power");
        assert!(accelerating.pitch.abs() <= PITCH_LIMIT + 0.2);

        // Roll magnitude now comes from the load the corner is actually putting
        // through the tyres, so a fabricated cornering state has to say how hard
        // it is cornering, not just which way.
        car.load_transfer = 0.6;
        car.yaw_rate = 1.0;
        let turning = pose_of(&car, track, 0.0);
        car.yaw_rate = -1.0;
        let other_way = pose_of(&car, track, 0.0);
        assert!(turning.roll > other_way.roll, "and the body leans with the turn");
        assert!((turning.roll - other_way.roll).abs() <= 2.0 * ROLL_LIMIT + 1.0e-4);
    }

    #[test]
    fn the_steering_pose_tracks_the_applied_steering() {
        let sim = racing();
        let mut car = *sim.car();
        car.steer = 1.0;
        assert!((pose_of(&car, sim.track(), 0.0).steer_angle - VISUAL_STEER_ANGLE).abs() < 1.0e-5);
        car.steer = -0.5;
        assert!(pose_of(&car, sim.track(), 0.0).steer_angle < 0.0);
    }

    /// The long-running stability requirement, driven through every behaviour
    /// the game has: acceleration, steering, braking, drift, boost, impact and
    /// reset — asserting state, not merely absence of a panic.
    #[test]
    fn a_long_scripted_run_stays_finite_and_inside_the_world() {
        let mut sim = racing();
        let track_length = sim.track().length();
        for i in 0..36_000u32 {
            let phase = (i / 300) % 7;
            let command = match phase {
                0 => DriveCommand::FLAT_OUT,
                1 => DriveCommand::turning(((i as f32) * 0.02).sin()),
                2 => DriveCommand { brake: 1.0, ..DriveCommand::IDLE },
                3 => DriveCommand { handbrake: true, ..DriveCommand::turning(0.9) },
                4 => DriveCommand { boost: true, ..DriveCommand::FLAT_OUT },
                5 => DriveCommand::turning(-1.0),
                _ => DriveCommand {
                    reset: i % 1_500 == 0,
                    ..DriveCommand::FLAT_OUT
                },
            };
            sim.step(command);

            let car = sim.car();
            assert!(car.is_finite(), "step {i} produced {car:?}");
            assert!(
                (0.0..=track_length + 1.0).contains(&car.distance),
                "step {i}: distance {} left the course",
                car.distance
            );
            let sample = sim.track().sample_at(car.distance);
            assert!(
                car.lateral.abs() <= sim.track().barrier_offset(&sample) + 0.1,
                "step {i}: lateral {} escaped the barriers",
                car.lateral
            );
            // The runaway guard. It used to be a flat 200 m/s, which was really
            // "the boosted ceiling, with room to spare"; there is no boosted
            // ceiling now, so the bound is derived from what one boost phase of
            // this script can actually add — full acceleration for the 300 steps
            // the phase lasts, on top of the clamp that still applies off boost.
            // Past that is the integrator running away, which is what this
            // watches for, rather than the car simply being fast.
            let v = sim.tuning().vehicle;
            let reachable = v.top_speed * controller::SPEED_HEADROOM
                + (v.accel + v.boost_accel_bonus) * 300.0 * DT;
            assert!(
                car.speed() <= reachable,
                "step {i}: speed {} ran away past the {reachable} m/s a boost phase can reach",
                car.speed()
            );
            let boost = sim.boost().charge();
            assert!((0.0..=1.0).contains(&boost), "step {i}: boost {boost}");
            let pose = sim.camera_pose(1.0);
            assert!(pose.eye.x.is_finite() && pose.eye.y.is_finite() && pose.eye.z.is_finite());
            assert!(pose.fov_degrees.is_finite());
        }
        assert!(sim.step_count() > 0);
    }

    #[test]
    fn events_are_drained_rather_than_accumulated() {
        let mut sim = RaceSim::shipping();
        sim.step(DriveCommand::IDLE);
        let drained = sim.take_events();
        assert!(sim.events().is_empty());
        sim.step(DriveCommand::IDLE);
        assert!(sim.events().len() <= drained.len() + 4, "no unbounded growth");
    }

    #[test]
    fn elapsed_time_is_a_step_count_not_a_clock() {
        let mut sim = racing();
        let before = sim.elapsed_seconds();
        for _ in 0..60 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        assert!(
            (sim.elapsed_seconds() - before - 1.0).abs() < 1.0e-3,
            "sixty steps is one second"
        );
    }
}
