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

// Camera constants: a PULLED-BACK OVER-THE-SHOULDER broadcast shot behind the
// kicker (KICKER_X=-0.7, KICKER_Z=12.6), the shot class the reference actually
// uses. The camera parks just above the kicker's head (y~2.1) and looks DOWN
// ~3.4 deg toward the goal mouth — a gentle broadcast tilt, not a bird's-eye.
//
// This corrects a FOREGROUND-LOOM miss. The previous pass sat the eye too close
// (z=KICKER_Z+9.9): the near ball ballooned to ~2x its reference size and the
// kicker's BOOTS fell off the bottom edge (~100% down, cropped), while the far
// goal was already well placed. Parking that close to the near, tall subjects
// blows up the whole foreground without gaining anything at the goal.
//
// The fix is a pure DOLLY-BACK + TELEPHOTO re-frame: dolly the eye back and
// narrow the lens. A first pass moved z=KICKER_Z+9.9 -> +14.9 at 16.0 deg, but
// residual FOREGROUND LOOM remained against the reference: the near kicker + ball
// still read markedly larger than the reference's moderate foreground, while the
// far goal read too small/narrow (keeper tiny) — the perspective was still too
// exaggerated (near/far ratio too high) versus the reference's flatter, telephoto
// broadcast compression. This pass pushes the SAME lever further: dolly the eye
// back +14.9 -> +22.9 (another ~8 m) and narrow the lens 16.0 -> 12.5 deg. The
// added distance drops the near/far ratio, so the near kicker + ball shrink
// (loom ~-15%) while the narrower FOV holds the FAR goal's angular size — net,
// the foreground de-looms and the goal/keeper grow relative to it, flattening the
// perspective toward the reference. Eye offset to +X (x=1.1) keeps the kicker
// (x=-0.7) in the LEFT third and the goal centered. Pure re-frame: EYE dollies
// back + lens narrows — no scene, pose, aim, or material change (target/up fixed).
pub const CAMERA_EYE: Vec3 = Vec3::new(1.1, 2.1, KICKER_Z + 22.9);
pub const CAMERA_TARGET: Vec3 = Vec3::new(0.1, 0.75, 4.5);
pub const CAMERA_FOV_Y_DEGREES: f32 = 12.5;
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
