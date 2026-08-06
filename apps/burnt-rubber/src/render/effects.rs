//! Speed streaks and impact sparks.
//!
//! All three are the same mechanism: a **bounded pool of boxes** whose positions
//! come from a deterministic phase advanced on the fixed step, not from a
//! particle simulation. There is no emitter, no lifetime list, no per-particle
//! integration and no allocation — a slot's position is a pure function of its
//! index and the current phase, so the whole system is a loop over a fixed array
//! writing transforms.
//!
//! That is not a shortcut, it is the correct amount of machinery for what these
//! do. Streaks exist to be blurred lines flicking past the camera at 300 km/h;
//! nobody will ever look at one for long enough to notice it did not follow a
//! ballistic arc. Spending a real particle system on them would buy nothing and
//! cost a per-frame allocation on the hot path.

use axiom::prelude::{Entity, Handle, Material, Mesh, RunningApp, Spawn, Transform, Vec3, Visible};
use axiom_math::Quat;

use crate::draw::Draw;
use crate::sim::car::CarState;
use crate::tuning::{VehicleTuning, DT};

use super::palette::ScenePalette;

/// Wind streaks flicking past the camera.
pub const STREAK_COUNT: usize = 64;
/// Impact sparks.
pub const SPARK_COUNT: usize = 28;

/// Speed below which no streaks are drawn at all (m/s). Streaks at walking pace
/// would read as snow, not speed.
pub const STREAK_ONSET: f32 = 34.0;

/// The pooled effect instances.
#[derive(Debug, Clone)]
pub struct Effects {
    streaks: Vec<Entity>,
    sparks: Vec<Entity>,
    /// Fixed per-slot offsets, drawn once at install — this is the only
    /// randomness in the whole effect system, and it never changes again.
    streak_seeds: Vec<Vec3>,
    spark_seeds: Vec<Vec3>,
    phase: f32,
    spark_life: f32,
    visible: usize,
}

impl Effects {
    /// Spawn every pooled effect, retired.
    pub fn install(app: &mut RunningApp, palette: &ScenePalette, seed: u64) -> Effects {
        let cube = app.add_mesh(Mesh::cube());
        let mut draw = Draw::seeded(seed).fork(EFFECT_SALT);
        let pool = |app: &mut RunningApp, count: usize, material: Handle<Material>| {
            (0..count)
                .map(|_| {
                    let e = app.spawn(Spawn::new(Transform::IDENTITY, cube, material));
                    app.set(e, Visible(false));
                    e
                })
                .collect::<Vec<_>>()
        };
        let seeds = |draw: &mut Draw, count: usize, spread: Vec3| {
            (0..count)
                .map(|_| {
                    Vec3::new(
                        draw.range(-spread.x, spread.x),
                        draw.range(0.0, spread.y),
                        draw.range(-spread.z, spread.z),
                    )
                })
                .collect::<Vec<_>>()
        };
        Effects {
            streaks: pool(app, STREAK_COUNT, palette.streak),
            sparks: pool(app, SPARK_COUNT, palette.spark),
            streak_seeds: seeds(&mut draw, STREAK_COUNT, Vec3::new(12.0, 2.4, 1.0)),
            spark_seeds: seeds(&mut draw, SPARK_COUNT, Vec3::new(1.0, 1.0, 1.0)),
            phase: 0.0,
            spark_life: 0.0,
            visible: 0,
        }
    }

    /// How many effect instances were drawn last frame.
    pub const fn visible_count(&self) -> usize {
        self.visible
    }

    /// Forget every transient — a new race, after which last race's sparks are
    /// not this race's. The per-slot seeds are untouched: they are
    /// drawn once at install and are part of the pool's identity, not its state.
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.spark_life = 0.0;
    }

    /// Advance the deterministic phase one fixed step.
    ///
    /// Called from the simulation side of the frame, not the render side, so a
    /// browser rendering at 144 Hz does not run the sparks four times as fast
    /// as one rendering at 60.
    pub fn step(&mut self, car: &CarState) {
        self.phase = (self.phase + DT).rem_euclid(PHASE_WRAP);
        self.spark_life = (self.spark_life - DT / SPARK_LIFETIME).max(0.0);
        if car.impact_strength > 0.0 && car.impact_steps > 0 {
            self.spark_life = self.spark_life.max(car.impact_strength);
        }
    }

    /// Pose every effect for this frame.
    pub fn pose(
        &mut self,
        app: &mut RunningApp,
        car: &CarState,
        eye: Vec3,
        forward: Vec3,
        tuning: &VehicleTuning,
    ) {
        self.visible = 0;
        self.pose_streaks(app, car, eye, forward, tuning);
        self.pose_sparks(app, car);
    }

    /// Streaks: short bright rods drawn in a volume ahead of the camera, moving
    /// backwards past it. Their length and count both scale with speed, so the
    /// effect arrives with the speed rather than being present at all times.
    fn pose_streaks(
        &mut self,
        app: &mut RunningApp,
        car: &CarState,
        eye: Vec3,
        forward: Vec3,
        tuning: &VehicleTuning,
    ) {
        let speed = car.speed();
        let intensity = ((speed - STREAK_ONSET) / (tuning.top_speed - STREAK_ONSET).max(1.0))
            .clamp(0.0, 1.0);
        let boost_bonus = if car.boosting { BOOST_STREAK_BONUS } else { 0.0 };
        let live = (((intensity + boost_bonus) * STREAK_COUNT as f32) as usize).min(STREAK_COUNT);
        let right = Vec3::UNIT_Y.cross(forward).normalize().unwrap_or(Vec3::UNIT_X);
        // Short rods, not long scratches. A rod aligned with the view direction
        // projects toward the vanishing point, so its *screen* length is much
        // greater than its world length - a 26 m streak reads as a scratch
        // across the frame rather than as motion. Keeping the world length
        // small is what keeps the projected streak a flick.
        let length = 1.4 + 4.2 * (intensity + boost_bonus).min(1.0);
        let rotation = Quat::from_euler_xyz(0.0, forward.x.atan2(forward.z), 0.0);

        for (index, entity) in self.streaks.iter().enumerate() {
            if index >= live {
                app.set(*entity, Visible(false));
                continue;
            }
            let seed = self.streak_seeds[index];
            // Each streak sweeps from far ahead to behind the camera on its own
            // offset cycle, so they do not pulse in unison.
            let cycle = (self.phase * STREAK_RATE + index as f32 * 0.137).rem_euclid(1.0);
            let along = STREAK_FAR - cycle * (STREAK_FAR - STREAK_NEAR);
            // Kept beside and just above the road rather than scattered through
            // the sky: streaks above the horizon are the classic tell of an
            // effect drawing in the wrong volume.
            let lateral = seed.x.signum() * (STREAK_INNER + seed.x.abs());
            let position = eye
                .add(forward.mul_scalar(along))
                .add(right.mul_scalar(lateral))
                .add(Vec3::new(0.0, seed.y - STREAK_DROP, 0.0));
            app.set(
                *entity,
                Transform::new(position, rotation, Vec3::new(0.05, 0.05, length)),
            );
            app.set(*entity, Visible(true));
            self.visible += 1;
        }
    }

    /// Sparks: a short-lived burst at the point of contact.
    fn pose_sparks(&mut self, app: &mut RunningApp, car: &CarState) {
        let life = self.spark_life;
        for (index, entity) in self.sparks.iter().enumerate() {
            if life <= 0.0 {
                app.set(*entity, Visible(false));
                continue;
            }
            let age = 1.0 - life;
            let seed = self.spark_seeds[index];
            let spread = seed.mul_scalar(1.0 + age * 6.0);
            let position = car
                .position
                .add(car.impact_direction.mul_scalar(-1.3))
                .add(spread)
                .add(Vec3::new(0.0, 0.5 - age * age * 2.0, 0.0));
            app.set(
                *entity,
                Transform::new(
                    position,
                    Quat::IDENTITY,
                    Vec3::ONE.mul_scalar(0.22 * life.max(0.05)),
                ),
            );
            app.set(*entity, Visible(true));
            self.visible += 1;
        }
    }
}

/// Salt separating the effect seeds from every other stream.
const EFFECT_SALT: u64 = 0x4D2B_7761_AC38_9E15;
/// How long the effect phase runs before wrapping (s). A long, non-round period
/// so nothing visibly repeats.
const PHASE_WRAP: f32 = 997.0;
/// How many full sweeps a streak makes per second of phase.
const STREAK_RATE: f32 = 1.9;
/// How far ahead of the camera a streak starts (m).
const STREAK_FAR: f32 = 55.0;
/// How far behind the camera a streak ends (m).
const STREAK_NEAR: f32 = -10.0;
/// Closest a streak comes to the camera's axis (m) - they belong in the
/// periphery, not across the road the player is trying to read.
const STREAK_INNER: f32 = 7.5;
/// How far below the eye a streak sits (m). Enough that the whole band stays
/// below the horizon, where motion belongs; streaks in the sky read as a
/// rendering fault, never as speed.
const STREAK_DROP: f32 = 2.6;
/// Extra streak intensity while boosting.
const BOOST_STREAK_BONUS: f32 = 0.45;
/// How long a spark burst lasts (s).
const SPARK_LIFETIME: f32 = 0.42;

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::prelude::{App, DefaultPlugins, Window};

    fn fixture() -> (RunningApp, Effects) {
        let mut app = App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build();
        let palette = ScenePalette::install(&mut app);
        let effects = Effects::install(&mut app, &palette, crate::DEFAULT_SEED);
        (app, effects)
    }

    fn car_at(speed: f32) -> CarState {
        let mut car = CarState::parked(Vec3::new(0.0, 0.0, 0.0), 0.0);
        car.forward_speed = speed;
        car
    }

    #[test]
    fn everything_starts_retired() {
        let (app, effects) = fixture();
        assert_eq!(effects.visible_count(), 0);
        for e in effects.streaks.iter().chain(&effects.sparks) {
            assert_eq!(app.get::<Visible>(*e), Some(Visible(false)));
        }
    }

    /// A new race starts with none of the last race's sparks in the
    /// air.
    #[test]
    fn resetting_clears_the_transients_but_not_the_pool() {
        let (mut app, mut effects) = fixture();
        let t = VehicleTuning::DEFAULT;
        let mut car = car_at(40.0);
        car.drifting = true;
        car.grounded = true;
        car.impact_strength = 0.9;
        car.impact_steps = 20;
        for _ in 0..30 {
            effects.step(&car);
        }
        effects.pose(&mut app, &car, Vec3::ZERO, Vec3::UNIT_Z, &t);
        assert!(effects.visible_count() > 0, "something was in the air");

        effects.reset();
        let quiet = car_at(0.0);
        effects.pose(&mut app, &quiet, Vec3::ZERO, Vec3::UNIT_Z, &t);
        assert_eq!(effects.visible_count(), 0, "and now nothing is");
    }

    #[test]
    fn streaks_arrive_with_speed_and_are_absent_at_a_crawl() {
        let (mut app, mut effects) = fixture();
        let t = VehicleTuning::DEFAULT;

        effects.pose(&mut app, &car_at(5.0), Vec3::ZERO, Vec3::UNIT_Z, &t);
        assert_eq!(effects.visible_count(), 0, "no streaks at walking pace");

        effects.pose(&mut app, &car_at(t.top_speed), Vec3::ZERO, Vec3::UNIT_Z, &t);
        let flat_out = effects.visible_count();
        assert!(flat_out > STREAK_COUNT / 2, "flat out is full of them: {flat_out}");

        effects.pose(&mut app, &car_at(t.top_speed * 0.6), Vec3::ZERO, Vec3::UNIT_Z, &t);
        let middling = effects.visible_count();
        assert!(middling < flat_out && middling > 0, "and it scales: {middling}");
    }

    #[test]
    fn boosting_adds_streaks_beyond_what_speed_alone_gives() {
        let (mut app, mut effects) = fixture();
        let t = VehicleTuning::DEFAULT;
        let mut car = car_at(t.top_speed * 0.7);
        effects.pose(&mut app, &car, Vec3::ZERO, Vec3::UNIT_Z, &t);
        let plain = effects.visible_count();
        car.boosting = true;
        effects.pose(&mut app, &car, Vec3::ZERO, Vec3::UNIT_Z, &t);
        assert!(effects.visible_count() > plain, "boost thickens the streaks");
    }

    #[test]
    fn streaks_are_placed_around_the_camera_not_the_car() {
        let (mut app, mut effects) = fixture();
        let t = VehicleTuning::DEFAULT;
        let eye = Vec3::new(100.0, 5.0, -200.0);
        effects.pose(&mut app, &car_at(t.top_speed), eye, Vec3::UNIT_Z, &t);
        let live: Vec<Vec3> = effects
            .streaks
            .iter()
            .filter(|e| app.get::<Visible>(**e) == Some(Visible(true)))
            .map(|e| app.get::<Transform>(*e).unwrap().translation)
            .collect();
        assert!(!live.is_empty());
        for p in live {
            assert!(
                p.distance(eye) < STREAK_FAR + 40.0,
                "a streak at {p:?} is nowhere near the camera at {eye:?}"
            );
            // Below the eye line, and out in the periphery: the two properties
            // that keep streaks reading as motion rather than as scratches.
            assert!(
                p.y <= eye.y,
                "a streak at {p:?} is above the camera at {eye:?}"
            );
            let lateral = p.subtract(eye).x.abs();
            assert!(
                lateral >= STREAK_INNER - 0.01,
                "a streak is only {lateral} m off the view axis"
            );
        }
    }

    #[test]
    fn an_impact_throws_sparks_that_die_away() {
        let (mut app, mut effects) = fixture();
        let t = VehicleTuning::DEFAULT;
        let mut car = car_at(60.0);
        car.impact_strength = 0.9;
        car.impact_steps = 20;
        effects.step(&car);
        effects.pose(&mut app, &car, Vec3::ZERO, Vec3::UNIT_Z, &t);
        assert!(
            effects
                .sparks
                .iter()
                .any(|e| app.get::<Visible>(*e) == Some(Visible(true))),
            "the hit sparks"
        );

        car.impact_strength = 0.0;
        car.impact_steps = 0;
        for _ in 0..(SPARK_LIFETIME / DT) as usize + 4 {
            effects.step(&car);
        }
        effects.pose(&mut app, &car, Vec3::ZERO, Vec3::UNIT_Z, &t);
        assert!(
            effects
                .sparks
                .iter()
                .all(|e| app.get::<Visible>(*e) == Some(Visible(false))),
            "and then they are gone"
        );
    }

    #[test]
    fn the_instance_count_is_bounded_by_the_pools() {
        let (mut app, mut effects) = fixture();
        let t = VehicleTuning::DEFAULT;
        let mut car = car_at(t.top_speed + t.boost_top_speed_bonus);
        car.boosting = true;
        car.drifting = true;
        car.grounded = true;
        car.impact_strength = 1.0;
        car.impact_steps = 30;
        for _ in 0..600 {
            effects.step(&car);
            effects.pose(&mut app, &car, Vec3::ZERO, Vec3::UNIT_Z, &t);
            assert!(
                effects.visible_count() <= STREAK_COUNT + SPARK_COUNT,
                "{} instances exceeds the pools",
                effects.visible_count()
            );
        }
    }

    /// The effects advance on the fixed step, so the same step sequence produces
    /// the same phase — a 144 Hz browser does not get faster sparks.
    #[test]
    fn the_effect_phase_is_deterministic_and_wraps() {
        let (mut app, _) = fixture();
        let palette = ScenePalette::install(&mut app);
        let run = |steps: usize| {
            let mut app = App::new()
                .window(Window::new(64, 64))
                .add_plugins(DefaultPlugins)
                .setup(|_, _, _| {})
                .build();
            let p = ScenePalette::install(&mut app);
            let mut e = Effects::install(&mut app, &p, crate::DEFAULT_SEED);
            let car = car_at(50.0);
            for _ in 0..steps {
                e.step(&car);
            }
            e.phase
        };
        assert_eq!(run(500), run(500));
        assert!(run(500) < PHASE_WRAP);
        let _ = palette;
    }

    #[test]
    fn the_per_slot_seeds_are_fixed_at_install_and_deterministic() {
        let (_, a) = fixture();
        let (_, b) = fixture();
        assert_eq!(a.streak_seeds, b.streak_seeds);
        assert_eq!(a.spark_seeds, b.spark_seeds);
        assert_eq!(a.streak_seeds.len(), STREAK_COUNT);
    }
}
