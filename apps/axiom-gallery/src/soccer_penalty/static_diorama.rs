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

// Camera constants: a NEAR-EYE-LEVEL OVER-THE-SHOULDER broadcast shot behind the
// kicker (KICKER_X=-0.7, KICKER_Z=12.6), the shot class the reference actually
// uses. The camera parks just above the kicker's head (y~2.1, KICKER head ~1.9)
// ~9.9 units back and looks DOWN only ~4.3 deg — a gentle tilt, not a steep
// bird's-eye. That low, near-level aim lets the WHOLE kicker stand tall in the
// LEFT third (hair ~30% down, boots ~89% down, boots clear of the bottom edge),
// with the ball on the grass mid-lower and the goal mouth + keeper riding the
// upper-middle.
//
// This corrects a prior OVER-ELEVATION miss. The previous pass lifted the eye
// high above the kicker's head (y=4.3) and tilted down ~9 deg, on the theory that
// only elevation opens the receding pitch. But looking DOWN that steeply onto the
// near kicker drops him to the BOTTOM of the frame and crops him at the torso:
// his feet fell to ~106% down (off the bottom edge), leaving only the upper body
// and hands visible while the #10, legs, and boots vanished. Elevation past the
// subject's own height doesn't "open the field" here — it just pushes the near,
// tall subject off the bottom.
//
// The fix is to DROP the eye back down to just above the kicker's head and aim
// NEARLY LEVEL (target y~0.75, so ~4.3 deg down). Geometrically this raises the
// near ground plane toward the horizon: the kicker's boots climb from ~106% down
// to ~89%, and his whole 1.9 m body rises into the frame (hair ~30%, boots ~89%)
// while the far goal — being ~22 units away — barely moves and stays framed with
// the crossbar in the upper third (~24% down) and the keeper's head ~30% down.
// The dolly-in from z=KICKER_Z+12.4 to +9.9 plus a hair-wider lens (18 -> 19.5)
// keep the kicker at reference SCALE without cropping. Eye offset to +X (x=1.1)
// holds the kicker (x=-0.7) in the left third. This is a pure re-frame: EYE drops
// + aim levels + a small dolly/FOV nudge — no scene, pose, or material change.
pub const CAMERA_EYE: Vec3 = Vec3::new(1.1, 2.1, KICKER_Z + 9.9);
pub const CAMERA_TARGET: Vec3 = Vec3::new(0.1, 0.75, 4.5);
pub const CAMERA_FOV_Y_DEGREES: f32 = 19.5;
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
