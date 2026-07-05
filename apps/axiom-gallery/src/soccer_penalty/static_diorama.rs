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

// Camera constants: an ELEVATED BROADCAST OVER-THE-SHOULDER shot behind the
// kicker (KICKER_X=-0.7, KICKER_Z=12.6), the shot class the reference actually
// uses. The reference camera sits ABOVE the kicker's head (~2.3) and looks DOWN
// ~10 deg, so the whole penalty-area ground plane reads with perspective: the
// full kicker from behind in the left third, the ball sitting on the grass
// mid-lower, and the goal mouth filling the upper third above it.
//
// The prior R4 strategy — an extreme telephoto (fov 15) parked at shoulder
// height (y=1.8) and dollied 14 units back — is the wrong shot class and misreads
// as a ground-level close-up: at shoulder height the near-level aim flattens the
// field plane to a thin edge-on band, and the narrow lens makes the near kicker
// loom and crop to just the legs (no torso, no #10, no head) while the ball
// oversizes. Lens compression cannot substitute for camera ELEVATION; the
// reference's readable field plane comes from looking down from above, not from a
// long throw.
//
// The elevation was right (it opens the ground plane the R4 shoulder-height
// telephoto flattened) but the WIDE fov was the miss: a 36-deg lens only ~4.6
// units behind the kicker makes the near kicker loom and CROP to head-and-
// shoulders while the far goal shrinks to a small distant rectangle (~44% of
// frame width). The reference has it both ways — an open ground plane AND a
// goal filling ~60% of the width with the crossbar in the upper third — which is
// ELEVATION *plus* TELEPHOTO, a pairing neither prior pass tried (R4 had the
// long lens but no lift; this pass had the lift but a wide lens).
//
// So: KEEP the eye lifted above the kicker's head (y~3.2), but DOLLY BACK to
// ~9.4 units behind him (eye z~22, not 17.2) and NARROW the lens to a telephoto
// broadcast fov (~18). The pullback+narrow is a depth compression: the far goal
// and keeper grow to reference scale while the near kicker shrinks from cropped-
// huge to a readable thigh-up silhouette. Eye offset to +X keeps the kicker
// (x=-0.7) in the left third; the target still aims low into the midground
// (y~1.05, z~4.5) so the ~7 deg downward tilt drops the ball into the lower
// third while the goal mouth rides the upper third.
pub const CAMERA_EYE: Vec3 = Vec3::new(0.9, 3.2, KICKER_Z + 9.4);
pub const CAMERA_TARGET: Vec3 = Vec3::new(-0.2, 1.05, 4.5);
pub const CAMERA_FOV_Y_DEGREES: f32 = 18.0;
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
