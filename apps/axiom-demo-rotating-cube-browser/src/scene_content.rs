//! The rotating-cube scene expressed as **data**.
//!
//! This is the app's content as a declarative value, not a sequence of
//! imperative engine calls. `CubeSliceDriver` interprets it generically each
//! tick: it spawns the nodes, attaches an engine `Spin` (so the rotation is
//! animated by the engine, not by app code), and registers the materials. A
//! different scene is a different `SceneContent` value — no code change.

use axiom_math::Vec3;

/// One cube: which axis it spins about, how far along x it sits, how many ticks
/// a full revolution takes, and its material colour (linear RGBA).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CubeSpec {
    pub spin_axis: Vec3,
    pub offset_x: f32,
    pub period_ticks: u32,
    pub color: [f32; 4],
}

/// The camera: how far back it sits on +z, and its perspective intrinsics.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CameraSpec {
    pub offset_z: f32,
    pub fovy_radians: f32,
    pub near: f32,
    pub far: f32,
}

/// The single directional light.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LightSpec {
    pub direction_world: Vec3,
    pub color: Vec3,
    pub intensity: f32,
}

/// A whole rotating-cube scene as data.
#[derive(Debug, Clone)]
pub(crate) struct SceneContent {
    pub clear_color: [f32; 4],
    pub cubes: Vec<CubeSpec>,
    pub camera: CameraSpec,
    pub light: LightSpec,
}

/// The demo scene: three cubes on distinct spin axes, a pulled-back camera, and
/// one white directional light. This value *is* the app's content.
pub(crate) fn demo_scene() -> SceneContent {
    SceneContent {
        clear_color: [0.05, 0.06, 0.08, 1.0],
        cubes: vec![
            CubeSpec {
                spin_axis: Vec3::UNIT_Y,
                offset_x: -2.6,
                period_ticks: 360,
                color: [0.85, 0.25, 0.25, 1.0], // red
            },
            CubeSpec {
                spin_axis: Vec3::UNIT_X,
                offset_x: 0.0,
                period_ticks: 360,
                color: [0.30, 0.80, 0.35, 1.0], // green
            },
            CubeSpec {
                spin_axis: Vec3::new(1.0, 1.0, 0.0),
                offset_x: 2.6,
                period_ticks: 360,
                color: [0.30, 0.50, 0.95, 1.0], // blue
            },
        ],
        camera: CameraSpec {
            offset_z: 8.0,
            fovy_radians: std::f32::consts::FRAC_PI_3,
            near: 0.1,
            far: 100.0,
        },
        light: LightSpec {
            direction_world: Vec3::new(0.3, -1.0, 0.4),
            color: Vec3::ONE,
            intensity: 1.0,
        },
    }
}
