//! Development-only field-paint inspection views.
//!
//! The paint system is camera-driven, so "does it look right" is not a single
//! question — it is a question per camera state. These are the six states the
//! paint has to stay stable and readable across. They are plain data: pure
//! camera poses over the field's own coordinate system, with no simulation
//! state in them, so the native tests can assert what each one *selects* and
//! `axiom-shot` can render what each one *looks like* from the same source.
//!
//! Rendering them:
//!
//! ```sh
//! cargo run -p axiom-shot -- --slice end-zone-field-gameplay
//! cargo run -p axiom-shot -- --slice end-zone-field-low-angle
//! cargo run -p axiom-shot -- --slice end-zone-field-yaw-left
//! cargo run -p axiom-shot -- --slice end-zone-field-yaw-right
//! cargo run -p axiom-shot -- --slice end-zone-field-far-end-zone
//! cargo run -p axiom-shot -- --slice end-zone-field-major-division
//! ```
//!
//! Nothing in the shipping app calls into this module; it exists so a change to
//! [`super::paint`] can be checked against every camera state it has to hold up
//! under, instead of against whichever one the play happened to be in.

use axiom::prelude::Vec3;

use crate::camera::CameraPose;

use super::coordinates::{FIELD_HALF_WIDTH, GOAL_LINE_Z};

/// The `Z` the inspection views are anchored at: a ten-yard major division well
/// inside the field, so both the division and the surrounding turf bands are in
/// every shot.
const ANCHOR_Z: f32 = -20.0;
/// The gameplay camera's eye height and set-back from the anchor, yards.
const EYE_HEIGHT: f32 = 7.5;
const EYE_BACK: f32 = 17.0;
/// The field of view the game plays at.
const VIEW_FOV: f32 = 46.0;

/// One inspection camera state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldView {
    /// The framing the game normally plays at: behind the play, looking
    /// downfield.
    Gameplay,
    /// Down at knee height, where the field plane is at its most oblique and
    /// thin geometry is least forgiving.
    LowAngle,
    /// The gameplay framing yawed hard left.
    YawLeft,
    /// The gameplay framing yawed hard right.
    YawRight,
    /// Looking the length of the field at the far end zone, so the whole
    /// near-to-far detail ramp is in one frame.
    FarEndZone,
    /// Parked right on top of a major division, where near-tier paint is at its
    /// largest on screen.
    MajorDivision,
}

/// Every inspection view, in a stable order.
pub const FIELD_VIEWS: [FieldView; 6] = [
    FieldView::Gameplay,
    FieldView::LowAngle,
    FieldView::YawLeft,
    FieldView::YawRight,
    FieldView::FarEndZone,
    FieldView::MajorDivision,
];

/// A camera at `eye` aimed `yaw` radians off downfield, `drop` yards below eye
/// level at the aim point.
fn aimed(eye: Vec3, yaw: f32, drop: f32) -> CameraPose {
    let reach = 30.0;
    CameraPose {
        eye,
        target: eye.add(Vec3::new(yaw.sin() * reach, -drop, yaw.cos() * reach)),
        fov_degrees: VIEW_FOV,
    }
}

impl FieldView {
    /// The registered `axiom-shot` slice name for this view.
    pub fn slice_name(self) -> &'static str {
        match self {
            FieldView::Gameplay => "end-zone-field-gameplay",
            FieldView::LowAngle => "end-zone-field-low-angle",
            FieldView::YawLeft => "end-zone-field-yaw-left",
            FieldView::YawRight => "end-zone-field-yaw-right",
            FieldView::FarEndZone => "end-zone-field-far-end-zone",
            FieldView::MajorDivision => "end-zone-field-major-division",
        }
    }

    /// This view's camera pose.
    pub fn camera(self) -> CameraPose {
        let base = Vec3::new(0.0, EYE_HEIGHT, ANCHOR_Z - EYE_BACK);
        let quarter = core::f32::consts::FRAC_PI_4;
        match self {
            FieldView::Gameplay => aimed(base, 0.0, 4.0),
            FieldView::LowAngle => aimed(Vec3::new(0.0, 1.1, ANCHOR_Z - EYE_BACK), 0.0, 0.2),
            FieldView::YawLeft => aimed(base, -quarter * 1.4, 4.0),
            FieldView::YawRight => aimed(base, quarter * 1.4, 4.0),
            FieldView::FarEndZone => aimed(
                Vec3::new(FIELD_HALF_WIDTH * 0.4, 12.0, -(GOAL_LINE_Z + 8.0)),
                0.0,
                7.0,
            ),
            FieldView::MajorDivision => aimed(
                Vec3::new(FIELD_HALF_WIDTH * 0.2, 2.4, ANCHOR_Z - 3.0),
                0.0,
                1.6,
            ),
        }
    }
}
