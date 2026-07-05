//! The Stage 1 static diorama: the deterministic aggregate of everything the
//! scene contains — the fixed camera, the Pass 3 style pass (light model + retro 32-bit
//! style descriptor), the ordered object list, and the static HUD.
//!
//! Nothing here reads wall-clock time or randomness. `StaticDiorama::stage1()`
//! is a pure function of the constants in this crate and returns an identical
//! value on every call.

use axiom_math::Vec3;

use crate::soccer_penalty::low_poly_assets::WORLD_UP;
use crate::soccer_penalty::penalty_hud::PenaltyHudModel;
use crate::soccer_penalty::penalty_scene::{build_penalty_objects, DioramaObject, KICKER_Z};
use crate::soccer_penalty::penalty_style_pass::PenaltyStylePass;

/// The fixed pinhole camera, parked behind the kicker and slightly elevated,
/// looking down `-Z` toward the goal. Stage 1 exposes no camera controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraConfig {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y_degrees: f32,
    pub near: f32,
    pub far: f32,
    pub aspect: f32,
}

// Camera constants (behind the kicker at KICKER_Z, at broadcast shoulder
// height, aimed at the goal mouth). The reference is a TELEPHOTO shot: the #10
// kicker reads FULL-BODY in the left third AND the goal holds a healthy size at
// the same time. That combination is only possible with lens compression — dolly
// the eye well back behind the kicker (~8.4 units, past the penalty box) and
// NARROW the FOV so the distant goal keeps its size while the near kicker shrinks
// from cropped-and-oversized to full-body. The eye stays at shoulder height
// (~1.8, roughly the kicker's head) rather than lifted: for a close, tall subject
// a raised eye tilts the feet off the bottom edge — distance, not height, is what
// reveals the full body here. Eye offset to +X keeps the kicker (x=-0.7) in the
// left third; a near-level aim at the goal mouth keeps the crossbar in the upper
// third and the keeper large.
// R4: the R3 telephoto (eye z=KICKER_Z+8.4, fov 21) left a goal/kicker distance
// ratio of ~2.5 -- still enough perspective divergence for the foreground kicker
// to loom over a shrunken goal. The reference's broadcast compression reads the
// near kicker and the far goal at comparable screen height, which needs a ratio
// ~1.9. So dolly the eye further back to ~14 units behind the kicker (eye z~26.6,
// ratio 26.6/14 = 1.9) and narrow the lens to fov ~15 so the dollied-back kicker
// still fills the frame full-body (~35% of the height) while the goal grows to
// fill more of the upper frame. Aim/height are unchanged; the longer throw only
// flattens the tilt further toward level.
pub const CAMERA_EYE: Vec3 = Vec3::new(1.2, 1.8, KICKER_Z + 14.0);
pub const CAMERA_TARGET: Vec3 = Vec3::new(-0.25, 0.7, 0.0);
pub const CAMERA_FOV_Y_DEGREES: f32 = 15.0;
pub const CAMERA_NEAR: f32 = 0.1;
pub const CAMERA_FAR: f32 = 120.0;
pub const CAMERA_ASPECT: f32 = 16.0 / 9.0;

impl CameraConfig {
    /// The fixed Stage 1 camera.
    pub const fn stage1() -> Self {
        Self {
            eye: CAMERA_EYE,
            target: CAMERA_TARGET,
            up: WORLD_UP,
            fov_y_degrees: CAMERA_FOV_Y_DEGREES,
            near: CAMERA_NEAR,
            far: CAMERA_FAR,
            aspect: CAMERA_ASPECT,
        }
    }

    /// The (un-normalized) forward direction from the eye toward the target.
    /// Uses [`Vec3`] arithmetic from the math layer.
    pub fn forward(&self) -> Vec3 {
        self.target.subtract(self.eye)
    }
}

/// The full static diorama: object list + camera + style pass + HUD.
#[derive(Debug, Clone, PartialEq)]
pub struct StaticDiorama {
    pub objects: Vec<DioramaObject>,
    pub camera: CameraConfig,
    /// Pass 3: the light model + retro 32-bit style descriptor the scene renders with.
    pub style_pass: PenaltyStylePass,
    /// The default (start-state) HUD; a live HUD is derived per frame from the
    /// interaction state (see `soccer_penalty_app`).
    pub hud: PenaltyHudModel,
}

impl StaticDiorama {
    /// Build the complete Stage 1 diorama from fixed constants.
    pub fn stage1() -> Self {
        Self {
            objects: build_penalty_objects(),
            camera: CameraConfig::stage1(),
            style_pass: PenaltyStylePass::stage1(),
            hud: PenaltyHudModel::stage1(),
        }
    }

    /// The stable, greppable labels of every object in build order.
    pub fn object_labels(&self) -> Vec<&'static str> {
        self.objects.iter().map(|o| o.label).collect()
    }
}

impl Default for StaticDiorama {
    fn default() -> Self {
        Self::stage1()
    }
}
