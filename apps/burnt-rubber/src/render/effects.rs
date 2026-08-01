//! Speed streaks, tyre smoke and impact sparks.
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
/// Tyre smoke puffs behind the rear wheels.
///
/// Deliberately few and deliberately small. The engine has no alpha blending —
/// `Material`'s opacity is carried but does not blend — so a "smoke puff" is an
/// **opaque** box. Anything bigger than a scuff at the tyre reads as a wall of
/// grey cubes chasing the car, which is worse than no smoke at all.
pub const SMOKE_COUNT: usize = 14;
/// Impact sparks.
pub const SPARK_COUNT: usize = 28;

/// Speed below which no streaks are drawn at all (m/s). Streaks at walking pace
/// would read as snow, not speed.
pub const STREAK_ONSET: f32 = 34.0;

/// The pooled effect instances.
#[derive(Debug, Clone)]
pub struct Effects {
    streaks: Vec<Entity>,
    smoke: Vec<Entity>,
    sparks: Vec<Entity>,
    /// Fixed per-slot offsets, drawn once at install — this is the only
    /// randomness in the whole effect system, and it never changes again.
    streak_seeds: Vec<Vec3>,
    smoke_seeds: Vec<Vec3>,
    spark_seeds: Vec<Vec3>,
    phase: f32,
    smoke_life: Vec<f32>,
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
            smoke: pool(app, SMOKE_COUNT, palette.smoke),
            sparks: pool(app, SPARK_COUNT, palette.spark),
            streak_seeds: seeds(&mut draw, STREAK_COUNT, Vec3::new(12.0, 2.4, 1.0)),
            smoke_seeds: seeds(&mut draw, SMOKE_COUNT, Vec3::new(0.34, 0.16, 0.30)),
            spark_seeds: seeds(&mut draw, SPARK_COUNT, Vec3::new(1.0, 1.0, 1.0)),
            phase: 0.0,
            smoke_life: vec![0.0; SMOKE_COUNT],
            spark_life: 0.0,
            visible: 0,
        }
    }

    /// How many effect instances were drawn last frame.
    pub const fn visible_count(&self) -> usize {
        self.visible
    }

    /// Advance the deterministic phase one fixed step.
    ///
    /// Called from the simulation side of the frame, not the render side, so a
    /// browser rendering at 144 Hz does not run the smoke four times as fast as
    /// one rendering at 60.
    pub fn step(&mut self, car: &CarState) {
        self.phase = (self.phase + DT).rem_euclid(PHASE_WRAP);
        // Smoke: each slot ages, and slots are re-lit while the car is sliding.
        let sliding = car.drifting && car.grounded;
        for (index, life) in self.smoke_life.iter_mut().enumerate() {
            *life = (*life - DT / SMOKE_LIFETIME).max(0.0);
            // One slot per step is re-lit, cycling through the pool, so a long
            // drift lays a continuous trail without any emitter bookkeeping.
            let slot = (self.phase / DT) as usize % SMOKE_COUNT.max(1);
            if sliding && slot == index {
                *life = 1.0;
            }
        }
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
        self.pose_smoke(app, car);
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

    /// Smoke: small scuffs at the rear contact patches, kept low and short.
    ///
    /// Emitted at the two rear wheels rather than as a cloud behind the car, and
    /// grown only a little, because these are opaque boxes (see
    /// [`SMOKE_COUNT`]): a puff that grows to the size of the car is a grey box
    /// the size of the car, sitting between the camera and the road.
    fn pose_smoke(&mut self, app: &mut RunningApp, car: &CarState) {
        let back = car.forward().mul_scalar(-REAR_AXLE_OFFSET);
        let right = car.right();
        for (index, entity) in self.smoke.iter().enumerate() {
            let life = self.smoke_life[index];
            if life <= 0.0 {
                app.set(*entity, Visible(false));
                continue;
            }
            let age = 1.0 - life;
            let seed = self.smoke_seeds[index];
            // Alternate wheels, so a slide lays a scuff either side.
            let side = if index % 2 == 0 { -1.0 } else { 1.0 };
            let size = SMOKE_SIZE + age * SMOKE_GROWTH;
            let position = car
                .position
                .add(back)
                .add(right.mul_scalar(side * REAR_TRACK_HALF))
                .add(seed.mul_scalar(1.0 + age))
                .add(Vec3::new(0.0, SMOKE_HEIGHT + age * SMOKE_RISE, 0.0));
            app.set(
                *entity,
                Transform::new(position, Quat::IDENTITY, Vec3::ONE.mul_scalar(size)),
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
/// How long a smoke puff lasts (s). Short: an opaque puff that lingers is a box
/// that lingers.
const SMOKE_LIFETIME: f32 = 0.55;
/// How far behind the car's centre the rear contact patches are (m).
const REAR_AXLE_OFFSET: f32 = 1.42;
/// Half the distance between the rear wheels (m).
const REAR_TRACK_HALF: f32 = 0.86;
/// A puff's size when it appears (m).
const SMOKE_SIZE: f32 = 0.16;
/// How much a puff grows over its life (m).
const SMOKE_GROWTH: f32 = 0.34;
/// How far off the ground a puff starts (m).
const SMOKE_HEIGHT: f32 = 0.14;
/// How far a puff drifts upward over its life (m).
const SMOKE_RISE: f32 = 0.30;
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
        for e in effects.streaks.iter().chain(&effects.smoke).chain(&effects.sparks) {
            assert_eq!(app.get::<Visible>(*e), Some(Visible(false)));
        }
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
    fn drifting_lays_smoke_and_it_fades_when_the_drift_ends() {
        let (mut app, mut effects) = fixture();
        let t = VehicleTuning::DEFAULT;
        let mut car = car_at(40.0);
        car.drifting = true;
        car.grounded = true;
        for _ in 0..SMOKE_COUNT * 2 {
            effects.step(&car);
        }
        effects.pose(&mut app, &car, Vec3::ZERO, Vec3::UNIT_Z, &t);
        let smoking = effects
            .smoke
            .iter()
            .filter(|e| app.get::<Visible>(**e) == Some(Visible(true)))
            .count();
        assert!(smoking > 0, "the drift smokes");

        car.drifting = false;
        for _ in 0..(SMOKE_LIFETIME / DT) as usize + 4 {
            effects.step(&car);
        }
        effects.pose(&mut app, &car, Vec3::ZERO, Vec3::UNIT_Z, &t);
        let after = effects
            .smoke
            .iter()
            .filter(|e| app.get::<Visible>(**e) == Some(Visible(true)))
            .count();
        assert_eq!(after, 0, "and it clears once the slide stops");
    }

    /// The engine draws these opaque, so their size is a correctness property,
    /// not a taste one: a puff bigger than a wheel is a grey box parked between
    /// the camera and the road.
    #[test]
    fn smoke_puffs_stay_small_and_low_and_at_the_wheels() {
        let (mut app, mut effects) = fixture();
        let t = VehicleTuning::DEFAULT;
        let mut car = car_at(40.0);
        car.drifting = true;
        car.grounded = true;
        for _ in 0..SMOKE_COUNT * 4 {
            effects.step(&car);
            effects.pose(&mut app, &car, Vec3::ZERO, Vec3::UNIT_Z, &t);
            for entity in &effects.smoke {
                if app.get::<Visible>(*entity) != Some(Visible(true)) {
                    continue;
                }
                let transform = app.get::<Transform>(*entity).expect("posed");
                assert!(
                    transform.scale.x <= SMOKE_SIZE + SMOKE_GROWTH + 1.0e-4,
                    "a puff grew to {} m",
                    transform.scale.x
                );
                assert!(
                    transform.scale.x < crate::render::car_model::CAR_WIDTH * 0.4,
                    "a puff is a scuff, not a box the size of the car"
                );
                let offset = transform.translation.subtract(car.position);
                assert!(
                    offset.y < 1.0,
                    "a puff is at wheel height, not over the roof: {}",
                    offset.y
                );
                assert!(
                    offset.length() < 3.0,
                    "a puff is at the wheels, not trailing the car: {}",
                    offset.length()
                );
            }
        }
        assert!(SMOKE_COUNT <= 16, "and there are few of them");
    }

    #[test]
    fn an_airborne_drift_lays_no_smoke() {
        let (mut app, mut effects) = fixture();
        let t = VehicleTuning::DEFAULT;
        let mut car = car_at(40.0);
        car.drifting = true;
        car.grounded = false;
        for _ in 0..SMOKE_COUNT * 2 {
            effects.step(&car);
        }
        effects.pose(&mut app, &car, Vec3::ZERO, Vec3::UNIT_Z, &t);
        assert_eq!(
            effects
                .smoke
                .iter()
                .filter(|e| app.get::<Visible>(**e) == Some(Visible(true)))
                .count(),
            0,
            "there is nothing to smoke against in the air"
        );
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
                effects.visible_count() <= STREAK_COUNT + SMOKE_COUNT + SPARK_COUNT,
                "{} instances exceeds the pools",
                effects.visible_count()
            );
        }
    }

    /// The effects advance on the fixed step, so the same step sequence produces
    /// the same phase — a 144 Hz browser does not get faster smoke.
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
        assert_eq!(a.smoke_seeds, b.smoke_seeds);
        assert_eq!(a.spark_seeds, b.spark_seeds);
        assert_eq!(a.streak_seeds.len(), STREAK_COUNT);
    }
}
