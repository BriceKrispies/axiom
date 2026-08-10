//! **Drawing boost pickups**: three pools, one per tier.
//!
//! # Why three pools and not one
//!
//! A node's material is bound when it is spawned, and the engine's component
//! vocabulary (`Transform`, `Bounds`, `Visible`) has no material in it — so a
//! pool of interchangeable nodes could never change what colour it draws. Three
//! pools, each installed with its tier's material, is the shape the engine
//! actually offers.
//!
//! It is also the shape that would be right anyway. The tier is the pickup's
//! identity, not a property of it: green *is* small. A single pool would have
//! meant either re-installing materials per frame (which is not a thing) or
//! giving up the colour, which is the whole feature.
//!
//! # Everything is installed at startup
//!
//! Non-negotiable, and the same constraint the traffic pool is under: the live
//! browser backend sizes its vertex and instance buffers from the mesh set
//! captured when the scene is built, so a body spawned later is a body that is
//! never drawn. [`PickupVisuals::install`] therefore spawns
//! `3 · PER_TIER_SLOTS` bodies up front, all hidden, and the frame's job is only
//! ever to place and reveal the ones in range.
//!
//! # The body
//!
//! Two parts, and each earns its place at a different distance:
//!
//! * a **diamond** — a cube spun 45° so its silhouette is a rhombus, floating at
//!   windscreen height. Nothing else on this road is a diamond, so at range,
//!   where the aerial-perspective term has washed the colour halfway to haze, the
//!   *shape* still says "pickup" before the hue does.
//! * a **ground marker** — a thin slab lying on the tarmac directly under it.
//!   The diamond floats, which makes it visible over a crest but useless for
//!   answering *which lane*; the marker is flat on the road and answers exactly
//!   that. Depth and lane, one object each.
//!
//! The diamond turns. Presentation only, driven by the simulation's step count
//! and the interpolation alpha, so it is smooth at any refresh rate and identical
//! on a replay — the same discipline the wheels and the traffic interpolation
//! keep.

use axiom::prelude::{Entity, Handle, Material, Mesh, RunningApp, Spawn, Transform, Visible};
use axiom_math::{Quat, Vec3};

use crate::course::pickups::BoostPickup;
#[cfg(test)]
use crate::course::specification::BoostTier;
use crate::track::Track;

use super::palette::ScenePalette;

/// How many pickups of one tier may be drawn at once.
///
/// Sized against the road rather than against the course: only what is inside
/// [`DRAW_DISTANCE_M`] can be drawn at all, and the densest thing an author can
/// write inside that is a row (bounded by
/// [`crate::course::specification::MAX_PICKUP_ROW`]) plus its neighbours. Six is
/// comfortably above what the shipping course ever presents at once, and three
/// pools of six is 36 bodies — a little over the traffic pool's 27.
pub const PER_TIER_SLOTS: usize = 6;

/// How far ahead of the car a pickup is drawn (m).
///
/// Short of the road's own draw distance on purpose. A pickup is a decision the
/// player makes about the next few seconds — at the expected speed this is a
/// little over eight seconds of warning, which is enough to change lane twice
/// and not so much that the road ahead is a wall of floating colour.
pub const DRAW_DISTANCE_M: f32 = 640.0;

/// How far behind the car a pickup keeps being drawn (m). Short: a pickup you
/// have passed is over, and one still glowing in the mirror reads as one you
/// missed.
pub const DRAW_BEHIND_M: f32 = 24.0;

/// How fast a diamond turns (rad/s).
///
/// Slow enough to read as *hovering* rather than as spinning debris, fast enough
/// that the changing silhouette catches the eye against a static road.
pub const SPIN_RATE: f32 = 1.7;

/// How high above the road the diamond floats (m).
///
/// Above the car's roofline (the chassis is about 1.5 m tall) so it is never
/// hidden behind the car in front, and below the tunnel ceiling. Not much above
/// it: at the chase camera's eye height a nearby diamond lands *on* the horizon,
/// where it is a bright speck against a bright sky rather than a shape against
/// the road.
pub const HOVER_HEIGHT_M: f32 = 2.0;

/// The diamond's edge length (m).
///
/// `Mesh::cube()` is a **unit** cube, so a `Transform` scale is the full size and
/// not a half-extent — the traffic bodies next door are scaled `2.05 x 1.05 x 4.4`
/// for a car about two metres wide. Authored at 0.62 on the assumption it was a
/// half-extent, the diamond came out the size of a wing mirror and read as
/// distant debris at any range; a little over a metre is the size that says
/// "object on the road" from a couple of hundred metres out.
const DIAMOND_M: f32 = 1.3;

/// The ground marker's dimensions (m): `x` across the lane, `y` its thickness,
/// `z` along the road.
///
/// Most of a lane wide (a lane is 3.5 m) and long enough along the road to still
/// be several pixels tall at range — a marker seen from behind is foreshortened
/// almost to nothing, so its *length* is what survives the perspective, not its
/// width.
const MARKER: Vec3 = Vec3::new(2.6, 0.08, 2.2);

/// How far above the road surface the marker sits (m).
///
/// A marker exactly *on* the surface is coplanar with the tarmac, and two
/// coplanar surfaces on a 0.35 m near plane shimmer from a couple of hundred
/// metres out — the same z-fighting the road's own four surfaces are separated
/// to avoid. Lifting it a few centimetres costs nothing at any viewing angle the
/// chase camera can produce.
const MARKER_LIFT_M: f32 = 0.05;

/// One pickup's two bodies.
#[derive(Debug, Clone, Copy)]
struct PickupParts {
    diamond: Entity,
    marker: Entity,
}

/// The pickup bodies: one pool per tier.
#[derive(Debug, Clone)]
pub struct PickupVisuals {
    /// Indexed by [`BoostTier::index`].
    tiers: [Vec<PickupParts>; 3],
}

impl PickupVisuals {
    /// Spawn `PER_TIER_SLOTS` bodies for each tier, all retired.
    pub fn install(app: &mut RunningApp, palette: &ScenePalette) -> PickupVisuals {
        let cube = app.add_mesh(Mesh::cube());
        let pool = |app: &mut RunningApp, material: Handle<Material>| {
            (0..PER_TIER_SLOTS)
                .map(|_| PickupParts {
                    diamond: retired(app, cube, material),
                    marker: retired(app, cube, material),
                })
                .collect::<Vec<PickupParts>>()
        };
        PickupVisuals {
            tiers: [
                pool(app, palette.pickup[0]),
                pool(app, palette.pickup[1]),
                pool(app, palette.pickup[2]),
            ],
        }
    }

    /// How many bodies one tier's pool holds.
    pub fn slots_per_tier(&self) -> usize {
        self.tiers[0].len()
    }

    /// Every body across every tier — what the diagnostics count.
    pub fn len(&self) -> usize {
        self.tiers.iter().map(Vec::len).sum()
    }

    /// Whether there are no bodies at all.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Place every pickup in range that this run has not taken, and retire the
    /// rest.
    ///
    /// `taken` answers "has this one been collected"; `phase` is the spin phase
    /// in radians, which the caller derives from the simulation clock so that
    /// this function stays a pure placement.
    ///
    /// Returns how many bodies were revealed, which is what the diagnostics
    /// overlay reports.
    pub fn pose(
        &self,
        app: &mut RunningApp,
        track: &Track,
        pickups: &[BoostPickup],
        car_distance: f32,
        phase: f32,
        taken: &impl Fn(&BoostPickup) -> bool,
    ) -> usize {
        // How many of each tier have been placed so far this frame — the pool
        // slot a pickup goes into is its position among the *visible* pickups of
        // its own tier, which is why this is a per-tier cursor and not the
        // pickup's index.
        let mut used = [0usize; 3];
        let near = car_distance - DRAW_BEHIND_M;
        let far = car_distance + DRAW_DISTANCE_M;

        for pickup in pickups {
            if (pickup.at_m < near) | (pickup.at_m > far) {
                continue;
            }
            if taken(pickup) {
                continue;
            }
            let tier = pickup.tier.index();
            let Some(parts) = self.tiers[tier].get(used[tier]) else {
                // The pool for this tier is full. Skipping is the right failure:
                // the pickups are walked in course order, so what is dropped is
                // the *furthest* one, which is the one the player can least act
                // on. See `PER_TIER_SLOTS` for why this is not expected to
                // happen on an authored course.
                continue;
            };
            used[tier] += 1;

            let sample = track.interpolated_at(pickup.at_m);
            let lateral = track.lane_lateral(&sample, pickup.lane);
            let ground = sample.at_lateral(lateral);
            let up = sample.up;

            // Each pickup turns at its own offset, so a row of three reads as
            // three objects rather than as one object drawn three times. The
            // offset is its distance along the course — a value it already has,
            // and one that cannot drift.
            let spin = phase + pickup.at_m * ROW_PHASE_RATE;
            app.set(
                parts.diamond,
                Transform::new(
                    ground.add(up.mul_scalar(HOVER_HEIGHT_M)),
                    // Tilted a quarter turn on Z so the silhouette is a rhombus,
                    // then turned about Y. Applying the tilt first is what keeps
                    // the diamond *point up* through the whole spin — the other
                    // order wobbles it end over end.
                    Quat::from_euler_xyz(0.0, spin, std::f32::consts::FRAC_PI_4),
                    Vec3::new(DIAMOND_M, DIAMOND_M, DIAMOND_M),
                ),
            );
            app.set(
                parts.marker,
                Transform::new(
                    ground.add(up.mul_scalar(MARKER_LIFT_M)),
                    // Flat on the road, turned to face along it. The marker does
                    // not spin: it is the thing that says *where*, and a turning
                    // one would be harder to line up on, not easier.
                    Quat::from_euler_xyz(0.0, sample.flat_forward().x.atan2(sample.flat_forward().z), 0.0),
                    MARKER,
                ),
            );
            app.set(parts.diamond, Visible(true));
            app.set(parts.marker, Visible(true));
        }

        // Retire every slot the frame did not use. Walked rather than tracked,
        // because a slot that is hidden twice costs nothing and a slot that is
        // never hidden is a pickup that hangs in the air after it is collected.
        for (tier, pool) in self.tiers.iter().enumerate() {
            for parts in &pool[used[tier].min(pool.len())..] {
                app.set(parts.diamond, Visible(false));
                app.set(parts.marker, Visible(false));
            }
        }
        used.iter().sum()
    }

    /// Hide everything — the start screen, and a frame with no race in it.
    pub fn hide(&self, app: &mut RunningApp) {
        for pool in &self.tiers {
            for parts in pool {
                app.set(parts.diamond, Visible(false));
                app.set(parts.marker, Visible(false));
            }
        }
    }
}

/// How much of a phase offset a metre of course adds to a diamond's spin
/// (rad/m).
///
/// Chosen so consecutive pickups in a row — 34 to 60 m apart — are visibly out
/// of step without the row looking like it is rippling.
const ROW_PHASE_RATE: f32 = 0.02;

/// The spin phase for a frame, from the simulation clock.
///
/// Kept out of [`PickupVisuals::pose`] so that placement is a pure function of
/// its inputs and the *only* thing reading a clock is one line. `alpha` is the
/// interpolation fraction through the current fixed step, so the spin is smooth
/// at 144 Hz and identical on a replay at 30.
pub fn spin_phase(step_count: u64, alpha: f32) -> f32 {
    (step_count as f32 + alpha.clamp(0.0, 1.0)) * crate::tuning::DT * SPIN_RATE
}

/// Spawn a part parked and invisible.
fn retired(app: &mut RunningApp, mesh: Handle<Mesh>, material: Handle<Material>) -> Entity {
    let entity = app.spawn(Spawn::new(Transform::IDENTITY, mesh, material));
    app.set(entity, Visible(false));
    entity
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::specification::PickupId;
    use crate::sim::RaceSim;
    use axiom::prelude::{App, DefaultPlugins, Window};

    fn app() -> RunningApp {
        App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build()
    }

    fn pickup(id: u32, at_m: f32, lane: i32, tier: BoostTier) -> BoostPickup {
        BoostPickup {
            id: PickupId(id),
            at_m,
            lane,
            tier,
            section: 0,
        }
    }

    #[test]
    fn every_tier_gets_its_own_pool_and_they_all_start_hidden() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let visuals = PickupVisuals::install(&mut app, &palette);
        assert_eq!(visuals.slots_per_tier(), PER_TIER_SLOTS);
        assert_eq!(visuals.len(), PER_TIER_SLOTS * 3);
        assert!(!visuals.is_empty());
        for pool in &visuals.tiers {
            for parts in pool {
                assert_eq!(app.get::<Visible>(parts.diamond), Some(Visible(false)));
                assert_eq!(app.get::<Visible>(parts.marker), Some(Visible(false)));
            }
        }
    }

    /// **The reason there are three pools.** Two pickups of different tiers
    /// drawn in the same frame must come out of different pools, or one of them
    /// would be the wrong colour.
    #[test]
    fn two_tiers_in_one_frame_use_their_own_pools() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let visuals = PickupVisuals::install(&mut app, &palette);
        let track = RaceSim::shipping().track().clone();
        let drawn = visuals.pose(
            &mut app,
            &track,
            &[
                pickup(0, 500.0, 0, BoostTier::Small),
                pickup(1, 520.0, 1, BoostTier::Large),
            ],
            480.0,
            0.0,
            &|_| false,
        );
        assert_eq!(drawn, 2);
        // Slot 0 of the small pool and slot 0 of the large pool, both visible.
        assert_eq!(
            app.get::<Visible>(visuals.tiers[0][0].diamond),
            Some(Visible(true))
        );
        assert_eq!(
            app.get::<Visible>(visuals.tiers[2][0].diamond),
            Some(Visible(true))
        );
        // And the medium pool, which had nothing to draw, is untouched.
        assert_eq!(
            app.get::<Visible>(visuals.tiers[1][0].diamond),
            Some(Visible(false))
        );
    }

    #[test]
    fn a_collected_pickup_is_not_drawn() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let visuals = PickupVisuals::install(&mut app, &palette);
        let track = RaceSim::shipping().track().clone();
        let one = [pickup(0, 500.0, 0, BoostTier::Medium)];

        assert_eq!(
            visuals.pose(&mut app, &track, &one, 480.0, 0.0, &|_| false),
            1
        );
        assert_eq!(
            app.get::<Visible>(visuals.tiers[1][0].diamond),
            Some(Visible(true))
        );
        // Collect it, and the body goes away on the very next frame.
        assert_eq!(
            visuals.pose(&mut app, &track, &one, 480.0, 0.0, &|_| true),
            0
        );
        assert_eq!(
            app.get::<Visible>(visuals.tiers[1][0].diamond),
            Some(Visible(false))
        );
    }

    #[test]
    fn only_pickups_in_range_are_drawn() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let visuals = PickupVisuals::install(&mut app, &palette);
        let track = RaceSim::shipping().track().clone();
        let here = 2_000.0;
        let set = [
            pickup(0, here - 200.0, 0, BoostTier::Small), // long behind
            pickup(1, here + 10.0, 0, BoostTier::Small),  // just ahead
            pickup(2, here + DRAW_DISTANCE_M + 50.0, 0, BoostTier::Small), // beyond
        ];
        assert_eq!(visuals.pose(&mut app, &track, &set, here, 0.0, &|_| false), 1);
    }

    /// A pool that runs out drops the *furthest* pickup, and does not panic.
    #[test]
    fn more_pickups_of_one_tier_than_slots_is_bounded_not_fatal() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let visuals = PickupVisuals::install(&mut app, &palette);
        let track = RaceSim::shipping().track().clone();
        let many: Vec<BoostPickup> = (0..PER_TIER_SLOTS as u32 + 4)
            .map(|k| pickup(k, 2_000.0 + k as f32 * 20.0, 0, BoostTier::Small))
            .collect();
        let drawn = visuals.pose(&mut app, &track, &many, 1_990.0, 0.0, &|_| false);
        assert_eq!(drawn, PER_TIER_SLOTS);
    }

    #[test]
    fn a_drawn_pickup_sits_over_its_own_lane_with_the_marker_beneath_it() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let visuals = PickupVisuals::install(&mut app, &palette);
        let track = RaceSim::shipping().track().clone();
        let at = 2_000.0;
        let lane = 1;
        visuals.pose(
            &mut app,
            &track,
            &[pickup(0, at, lane, BoostTier::Large)],
            at - 100.0,
            0.0,
            &|_| false,
        );
        let sample = track.interpolated_at(at);
        let expected = sample.at_lateral(track.lane_lateral(&sample, lane));

        let diamond = app.get::<Transform>(visuals.tiers[2][0].diamond).unwrap();
        let marker = app.get::<Transform>(visuals.tiers[2][0].marker).unwrap();
        // The marker is on the road, the diamond is above it, and they are over
        // the same point.
        assert!(marker.translation.distance(expected) < 0.2, "{marker:?}");
        assert!(
            (diamond.translation.y - marker.translation.y - HOVER_HEIGHT_M).abs() < 0.2,
            "the diamond is not hovering over its marker"
        );
        let flat = Vec3::new(
            diamond.translation.x - marker.translation.x,
            0.0,
            diamond.translation.z - marker.translation.z,
        );
        assert!(flat.length() < 0.3, "the diamond drifted off its marker");
    }

    /// The spin is a function of the clock and nothing else, and it is smooth
    /// across a fixed step rather than jumping at the boundary.
    #[test]
    fn the_spin_phase_advances_smoothly_with_the_simulation_clock() {
        assert_eq!(spin_phase(0, 0.0), 0.0);
        let a = spin_phase(100, 0.0);
        let b = spin_phase(100, 0.5);
        let c = spin_phase(101, 0.0);
        assert!(a < b && b < c, "{a} {b} {c}");
        // Halfway through a step is halfway to the next step's value.
        assert!((b - (a + c) * 0.5).abs() < 1.0e-5);
        // Out-of-range alpha is clamped rather than throwing the spin.
        assert_eq!(spin_phase(100, 9.0), c);
        assert_eq!(spin_phase(100, -9.0), a);
    }

    #[test]
    fn hiding_everything_retires_every_body() {
        let mut app = app();
        let palette = ScenePalette::install(&mut app);
        let visuals = PickupVisuals::install(&mut app, &palette);
        let track = RaceSim::shipping().track().clone();
        visuals.pose(
            &mut app,
            &track,
            &[pickup(0, 500.0, 0, BoostTier::Small)],
            480.0,
            0.0,
            &|_| false,
        );
        visuals.hide(&mut app);
        for pool in &visuals.tiers {
            for parts in pool {
                assert_eq!(app.get::<Visible>(parts.diamond), Some(Visible(false)));
                assert_eq!(app.get::<Visible>(parts.marker), Some(Visible(false)));
            }
        }
    }
}
