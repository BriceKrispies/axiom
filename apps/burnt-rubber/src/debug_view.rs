//! The visual debugging overlay: the course's internals, drawn as markers.
//!
//! This exists to answer the questions that a screenshot cannot: *is the
//! centreline where I think it is, are the sample frames orthonormal, where does
//! this chunk end, which way is the car actually travelling, where would a reset
//! put me.* It is a development tool, and it is deliberately built as a
//! **separate pooled marker set** that is invisible unless explicitly enabled —
//! not as a mode that changes how the ordinary scene is drawn.
//!
//! Keeping it out of the normal presentation path is the point. A debug view
//! that shares state with the shipping renderer eventually becomes a debug view
//! that *changes* the shipping renderer, and then it is not a debug view any
//! more. Here, turning it off hides some boxes and nothing else.

use axiom::prelude::{Entity, Material, Mesh, RunningApp, Spawn, Transform, Vec3, Visible};
use axiom_math::Quat;

use crate::render::palette;
use crate::sim::RaceSim;

/// What a debug marker is showing. Each kind is one material, so the whole
/// overlay is a handful of draw calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    /// The sampled centreline.
    Centreline,
    /// A sample's local frame (its right vector).
    SampleFrame,
    /// A rendered chunk boundary.
    ChunkBoundary,
    /// The car's velocity vector.
    Velocity,
    /// The car's chassis forward vector.
    Forward,
    /// The chase camera's look target.
    CameraTarget,
    /// The reset point the car would return to.
    ResetPoint,
    /// A traffic car's lane path.
    TrafficPath,
}

impl MarkerKind {
    /// Every kind, in a stable order.
    pub const ALL: [MarkerKind; 8] = [
        MarkerKind::Centreline,
        MarkerKind::SampleFrame,
        MarkerKind::ChunkBoundary,
        MarkerKind::Velocity,
        MarkerKind::Forward,
        MarkerKind::CameraTarget,
        MarkerKind::ResetPoint,
        MarkerKind::TrafficPath,
    ];

    /// How many markers of this kind the pool holds.
    pub const fn pool_capacity(self) -> usize {
        match self {
            MarkerKind::Centreline => 160,
            MarkerKind::SampleFrame => 40,
            MarkerKind::ChunkBoundary => 20,
            MarkerKind::Velocity => 12,
            MarkerKind::Forward => 12,
            MarkerKind::CameraTarget => 2,
            MarkerKind::ResetPoint => 2,
            MarkerKind::TrafficPath => 120,
        }
    }

    /// The marker colour.
    pub const fn color(self) -> [f32; 3] {
        match self {
            MarkerKind::Centreline => [0.20, 0.85, 0.98],
            MarkerKind::SampleFrame => [0.95, 0.35, 0.85],
            MarkerKind::ChunkBoundary => [0.98, 0.92, 0.20],
            MarkerKind::Velocity => [0.25, 0.95, 0.35],
            MarkerKind::Forward => [0.98, 0.55, 0.12],
            MarkerKind::CameraTarget => [0.95, 0.15, 0.15],
            MarkerKind::ResetPoint => [0.55, 0.35, 0.98],
            MarkerKind::TrafficPath => [0.60, 0.62, 0.70],
        }
    }
}

/// One marker to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marker {
    pub kind: MarkerKind,
    pub position: Vec3,
    pub yaw: f32,
    pub scale: Vec3,
}

/// The pooled debug overlay.
#[derive(Debug, Clone)]
pub struct DebugView {
    pools: Vec<(MarkerKind, Vec<Entity>)>,
    markers: Vec<Marker>,
    enabled: bool,
}

impl DebugView {
    /// Spawn every marker pool, retired.
    pub fn install(app: &mut RunningApp) -> DebugView {
        let cube = app.add_mesh(Mesh::cube());
        let pools = MarkerKind::ALL
            .iter()
            .map(|kind| {
                let c = kind.color();
                let material = app.add_material(
                    Material::lit(palette::rgb(c[0], c[1], c[2]))
                        .with_emissive(palette::rgb(c[0] * 0.6, c[1] * 0.6, c[2] * 0.6)),
                );
                let entities = (0..kind.pool_capacity())
                    .map(|_| {
                        let e = app.spawn(Spawn::new(Transform::IDENTITY, cube, material));
                        app.set(e, Visible(false));
                        e
                    })
                    .collect();
                (*kind, entities)
            })
            .collect();
        DebugView {
            pools,
            markers: Vec::new(),
            enabled: false,
        }
    }

    /// Whether the overlay is showing.
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Show or hide the overlay.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// The markers built for the last update.
    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// Rebuild and pose the overlay for the current simulation state.
    pub fn update(&mut self, app: &mut RunningApp, sim: &RaceSim) {
        self.markers.clear();
        if self.enabled {
            build_markers(sim, &mut self.markers);
        }
        for (kind, entities) in &self.pools {
            let mut slot = 0usize;
            for marker in self.markers.iter().filter(|m| m.kind == *kind) {
                let Some(entity) = entities.get(slot) else {
                    break;
                };
                app.set(
                    *entity,
                    Transform::new(
                        marker.position,
                        Quat::from_euler_xyz(0.0, marker.yaw, 0.0),
                        marker.scale,
                    ),
                );
                app.set(*entity, Visible(true));
                slot += 1;
            }
            for entity in entities.iter().skip(slot) {
                app.set(*entity, Visible(false));
            }
        }
    }
}

/// How far ahead and behind the car the overlay draws course internals (m).
pub const MARKER_REACH: f32 = 300.0;

/// Build the marker set for the current state.
///
/// A pure function of the simulation, so the overlay is exactly as
/// deterministic as everything else and can be asserted on directly.
pub fn build_markers(sim: &RaceSim, out: &mut Vec<Marker>) {
    let track = sim.track();
    let car = sim.car();
    let from = (car.distance - MARKER_REACH * 0.2).max(0.0);
    let to = (car.distance + MARKER_REACH).min(track.length());

    // The centreline, and every eighth sample's right vector.
    let step = track.spacing() * 4.0;
    let count = ((to - from) / step).floor().max(0.0) as usize;
    for i in 0..count.min(MarkerKind::Centreline.pool_capacity()) {
        let sample = track.interpolated_at(from + i as f32 * step);
        out.push(Marker {
            kind: MarkerKind::Centreline,
            position: sample.position.add(Vec3::new(0.0, 0.35, 0.0)),
            yaw: sample.heading,
            scale: Vec3::new(0.2, 0.2, 1.4),
        });
        if i % 8 == 0 {
            out.push(Marker {
                kind: MarkerKind::SampleFrame,
                position: sample
                    .at_lateral(sample.half_width)
                    .add(Vec3::new(0.0, 0.6, 0.0)),
                yaw: sample.heading,
                scale: Vec3::new(0.4, 1.2, 0.4),
            });
        }
    }

    // Chunk boundaries, as tall posts across the road.
    let chunk_length = crate::render::road_mesh::CHUNK_LENGTH;
    let first_chunk = (from / chunk_length).ceil() as usize;
    for i in 0..MarkerKind::ChunkBoundary.pool_capacity() {
        let distance = (first_chunk + i) as f32 * chunk_length;
        if distance > to {
            break;
        }
        let sample = track.sample_at(distance);
        out.push(Marker {
            kind: MarkerKind::ChunkBoundary,
            position: sample.position.add(Vec3::new(0.0, 3.0, 0.0)),
            yaw: sample.heading,
            scale: Vec3::new(sample.half_width * 2.0, 0.3, 0.3),
        });
    }

    // The car's vectors: where it points, and where it is going.
    let velocity = car.heading_of_travel();
    out.push(Marker {
        kind: MarkerKind::Velocity,
        position: car
            .position
            .add(velocity.mul_scalar(VECTOR_LENGTH * 0.5))
            .add(Vec3::new(0.0, 1.8, 0.0)),
        yaw: velocity.x.atan2(velocity.z),
        scale: Vec3::new(0.2, 0.2, VECTOR_LENGTH),
    });
    let forward = car.forward();
    out.push(Marker {
        kind: MarkerKind::Forward,
        position: car
            .position
            .add(forward.mul_scalar(VECTOR_LENGTH * 0.5))
            .add(Vec3::new(0.0, 2.1, 0.0)),
        yaw: car.yaw,
        scale: Vec3::new(0.16, 0.16, VECTOR_LENGTH),
    });

    // The camera's look target, and the reset point.
    let camera = sim.camera_pose(1.0);
    out.push(Marker {
        kind: MarkerKind::CameraTarget,
        position: camera.target,
        yaw: 0.0,
        scale: Vec3::ONE.mul_scalar(0.6),
    });
    let reset = track.safe_reset(car.distance);
    out.push(Marker {
        kind: MarkerKind::ResetPoint,
        position: reset.position.add(Vec3::new(0.0, 1.4, 0.0)),
        yaw: reset.heading,
        scale: Vec3::new(0.8, 2.8, 0.8),
    });

    // Traffic lane paths: a short run of the lane each live car is holding.
    for car in sim.traffic().active() {
        for ahead in 0..TRAFFIC_PATH_SEGMENTS {
            let distance = car.distance + ahead as f32 * TRAFFIC_PATH_SPACING;
            if distance > track.length() {
                break;
            }
            let sample = track.interpolated_at(distance);
            out.push(Marker {
                kind: MarkerKind::TrafficPath,
                position: sample.at_lateral(car.lateral).add(Vec3::new(0.0, 0.5, 0.0)),
                yaw: sample.heading,
                scale: Vec3::new(0.16, 0.16, 1.0),
            });
        }
    }
}

/// Length of a car vector marker (m).
const VECTOR_LENGTH: f32 = 6.0;
/// How many segments of lane path are drawn per traffic car.
const TRAFFIC_PATH_SEGMENTS: usize = 4;
/// Spacing of traffic path segments (m).
const TRAFFIC_PATH_SPACING: f32 = 6.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DriveCommand;
    use crate::sim::RacePhase;
    use axiom::prelude::{App, DefaultPlugins, Window};

    fn fixture() -> (RunningApp, RaceSim, DebugView) {
        let mut sim = RaceSim::shipping();
        while sim.phase() == RacePhase::Countdown {
            sim.step(DriveCommand::IDLE);
        }
        crate::script::drive_autopilot(&mut sim, 900);
        let mut app = App::new()
            .window(Window::new(320, 200))
            .add_plugins(DefaultPlugins)
            .setup(|_, _, _| {})
            .build();
        let view = DebugView::install(&mut app);
        (app, sim, view)
    }

    #[test]
    fn the_overlay_is_off_and_invisible_until_enabled() {
        let (mut app, sim, mut view) = fixture();
        assert!(!view.enabled());
        view.update(&mut app, &sim);
        assert!(view.markers().is_empty());
        for (_, entities) in &view.pools {
            for e in entities {
                assert_eq!(app.get::<Visible>(*e), Some(Visible(false)));
            }
        }
    }

    #[test]
    fn enabling_draws_every_kind_of_marker() {
        let (mut app, sim, mut view) = fixture();
        view.set_enabled(true);
        view.update(&mut app, &sim);
        assert!(!view.markers().is_empty());

        let mut seen: Vec<MarkerKind> = Vec::new();
        for marker in view.markers() {
            if !seen.contains(&marker.kind) {
                seen.push(marker.kind);
            }
        }
        for kind in MarkerKind::ALL {
            assert!(seen.contains(&kind), "{kind:?} was never drawn");
        }
    }

    #[test]
    fn disabling_hides_everything_again() {
        let (mut app, sim, mut view) = fixture();
        view.set_enabled(true);
        view.update(&mut app, &sim);
        assert!(view
            .pools
            .iter()
            .any(|(_, e)| e.iter().any(|x| app.get::<Visible>(*x) == Some(Visible(true)))));

        view.set_enabled(false);
        view.update(&mut app, &sim);
        for (_, entities) in &view.pools {
            for e in entities {
                assert_eq!(app.get::<Visible>(*e), Some(Visible(false)));
            }
        }
    }

    #[test]
    fn markers_are_bounded_by_their_pools() {
        let (mut app, mut sim, mut view) = fixture();
        view.set_enabled(true);
        for _ in 0..60 {
            for _ in 0..30 {
                sim.step(DriveCommand::FLAT_OUT);
            }
            view.update(&mut app, &sim);
            for kind in MarkerKind::ALL {
                let drawn = view
                    .pools
                    .iter()
                    .find(|(k, _)| *k == kind)
                    .map(|(_, e)| {
                        e.iter()
                            .filter(|x| app.get::<Visible>(**x) == Some(Visible(true)))
                            .count()
                    })
                    .unwrap_or(0);
                assert!(
                    drawn <= kind.pool_capacity(),
                    "{kind:?}: {drawn} drawn, pool holds {}",
                    kind.pool_capacity()
                );
            }
        }
    }

    #[test]
    fn the_marker_set_is_a_pure_function_of_the_simulation() {
        let (_, sim, _) = fixture();
        let mut a = Vec::new();
        let mut b = Vec::new();
        build_markers(&sim, &mut a);
        build_markers(&sim, &mut b);
        assert_eq!(a, b);
        assert!(a.iter().all(|m| m.position.x.is_finite()
            && m.position.y.is_finite()
            && m.position.z.is_finite()
            && m.yaw.is_finite()));
    }

    #[test]
    fn the_vectors_point_where_the_car_points_and_goes() {
        let (_, mut sim, _) = fixture();
        // Establish a slide so the two vectors genuinely differ.
        for _ in 0..40 {
            sim.step(DriveCommand {
                handbrake: true,
                ..DriveCommand::turning(1.0)
            });
        }
        let mut markers = Vec::new();
        build_markers(&sim, &mut markers);
        let forward = markers.iter().find(|m| m.kind == MarkerKind::Forward).unwrap();
        let velocity = markers.iter().find(|m| m.kind == MarkerKind::Velocity).unwrap();
        assert!((forward.yaw - sim.car().yaw).abs() < 1.0e-4);
        let travel = sim.car().heading_of_travel();
        assert!((velocity.yaw - travel.x.atan2(travel.z)).abs() < 1.0e-4);
        assert!(sim.car().drifting, "the test exercised the case it meant to");
    }

    #[test]
    fn the_reset_marker_is_where_a_reset_would_put_the_car() {
        let (_, sim, _) = fixture();
        let mut markers = Vec::new();
        build_markers(&sim, &mut markers);
        let reset = markers
            .iter()
            .find(|m| m.kind == MarkerKind::ResetPoint)
            .expect("drawn");
        let expected = sim.track().safe_reset(sim.car().distance);
        assert!(reset.position.distance(expected.position) < 2.0);
    }

    #[test]
    fn chunk_boundary_markers_land_on_chunk_boundaries() {
        let (_, sim, _) = fixture();
        let mut markers = Vec::new();
        build_markers(&sim, &mut markers);
        let length = crate::render::road_mesh::CHUNK_LENGTH;
        for marker in markers.iter().filter(|m| m.kind == MarkerKind::ChunkBoundary) {
            let (distance, _) = sim
                .track()
                .localise(marker.position, sim.car().distance, 400.0);
            let remainder = distance.rem_euclid(length);
            assert!(
                remainder < 3.0 || remainder > length - 3.0,
                "a boundary marker at {distance} m is not on a boundary"
            );
        }
    }

    #[test]
    fn every_kind_declares_a_pool_and_a_distinct_colour() {
        let colours: Vec<[f32; 3]> = MarkerKind::ALL.iter().map(|k| k.color()).collect();
        for (i, a) in colours.iter().enumerate() {
            assert!(MarkerKind::ALL[i].pool_capacity() > 0);
            for b in colours.iter().skip(i + 1) {
                assert_ne!(a, b, "marker {i} shares a colour");
            }
        }
    }
}
