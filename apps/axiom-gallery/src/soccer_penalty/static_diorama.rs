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
// The fix is a pure DOLLY + TELEPHOTO re-frame: dolly the eye and narrow the lens.
// A first pass moved z=KICKER_Z+9.9 -> +14.9 at 16.0 deg, then a de-loom pass went
// further to +22.9 at 12.5 deg to flatten the perspective.
//
// SCALE correction (art-director lens): that de-loom pass OVER-shot. At +22.9 the
// hero over-the-shoulder kicker — the subject that defines this shot class — reads
// far too small: head ~36% / boots ~72%, spanning only ~36% of frame height, versus
// the reference's LARGE foreground kicker at head ~29% / boots ~88% (~59% span). The
// ball likewise sat ~63% down (ref ~74%) and the goal ~53% wide (ref ~64%): the whole
// foreground had been shrunk into a flatter, wronger shot. The single-lever fix is a
// DOLLY-IN, restoring the eye distance from +22.9 to +15.0 while KEEPING the 12.5 deg
// telephoto lens (so the goal stays sized as the narrow lens holds it). Coming closer
// re-enlarges the near kicker + ball faster than the far goal (restoring the
// reference's foreground presence): kicker head ~32% / boots ~87% (ref 29/88), ball
// ~72% down (ref 74), goal-line ~58% (ref 57), goal ~69% wide. Eye offset to +X
// (x=1.1) keeps the kicker (x=-0.7) in the LEFT third and the goal centered. One
// lever: EYE dollies in — no lens, scene, pose, aim, or material change (target/up
// fixed).
pub const CAMERA_EYE: Vec3 = Vec3::new(1.1, 2.1, KICKER_Z + 15.0);
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
