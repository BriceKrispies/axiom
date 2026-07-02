//! The app facade: the single entry point that assembles the diorama, its
//! render plan, and the HUD.
//!
//! This is the composition root for the app. It reads the static diorama
//! descriptor (`static_diorama`), overlays the live ball pose (Pass 5) onto the
//! ball + ball-shadow objects and appends any trail samples, derives the HUD
//! from the interaction state (Pass 4), and produces the ordered render plan.

use axiom_math::Vec3;

use crate::soccer_penalty::low_poly_assets::PrimitiveShape;
use crate::soccer_penalty::penalty_ball::PenaltyBallPose;
use crate::soccer_penalty::penalty_effects::{PenaltyEffectDescriptor, PenaltyGoalFramePart};
use crate::soccer_penalty::penalty_goalie::{
    PenaltyGoalieContactKind, PenaltyGoalieDebugDescriptor, PenaltyGoalieVolume,
    PenaltyGoalieVolumeKind, PenaltyGoalieVolumeSet, PenaltyGoalieVolumeShape,
};
use crate::soccer_penalty::penalty_goalie_pose::{PenaltyGoaliePartKind, PenaltyGoaliePoseDescriptor};
use crate::soccer_penalty::penalty_hud::PenaltyHudModel;
use crate::soccer_penalty::penalty_interaction::PenaltyInteractionState;
use crate::soccer_penalty::penalty_materials::PenaltyMaterialId;
use crate::soccer_penalty::penalty_render_plan::PenaltyRenderPlan;
use crate::soccer_penalty::penalty_scene::{DioramaObject, DioramaRole, ObjectId, BALL_RADIUS};
use crate::soccer_penalty::penalty_session::PenaltySessionState;
use crate::soccer_penalty::static_diorama::{CameraConfig, StaticDiorama};

/// The bundled, deterministic per-frame artifacts.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage1Diorama {
    /// The diorama objects in stable order (ball/shadow reflect the ball pose;
    /// trail samples appended during flight).
    pub objects: Vec<DioramaObject>,
    /// The ordered, backend-neutral render plan (sorted draw list + camera +
    /// style pass).
    pub render_plan: PenaltyRenderPlan,
    /// The arcade HUD for this frame's interaction state.
    pub hud: PenaltyHudModel,
}

/// The soccer penalty-kick app.
///
/// Through Pass 5 the app builds a fixed static diorama, a deterministic
/// aim/power HUD, and a deterministic ball flight. It is a unit struct so the
/// entry point reads as `SoccerPenaltyApp::build_stage1()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SoccerPenaltyApp;

impl SoccerPenaltyApp {
    /// Build the diorama for the default (start) interaction state.
    pub fn build_stage1() -> Stage1Diorama {
        Self::build_frame(&PenaltyInteractionState::start())
    }

    /// Build the diorama for a specific interaction state, with goalie-volume
    /// debug visualization **off**. Deterministic in `state`.
    pub fn build_frame(state: &PenaltyInteractionState) -> Stage1Diorama {
        Self::build_frame_with_debug(state, PenaltyGoalieDebugDescriptor::DISABLED)
    }

    /// Build the diorama for a specific interaction state, optionally overlaying
    /// the goalie save-volume debug markers. When `debug` is disabled the result
    /// is byte-identical to [`Self::build_frame`] (no debug items, no HUD debug
    /// label) — debug never affects contact detection or gameplay.
    pub fn build_frame_with_debug(
        state: &PenaltyInteractionState,
        debug: PenaltyGoalieDebugDescriptor,
    ) -> Stage1Diorama {
        let diorama = StaticDiorama::stage1();
        let pose = state.ball_pose();
        let goalie_pose = state.goalie.descriptor();

        // Overlay the live ball pose and the sampled goalie pose onto the static
        // ball / shadow / goalie objects (identity at rest → default unchanged).
        let mut objects: Vec<DioramaObject> = diorama
            .objects
            .iter()
            .map(|o| apply_goalie_pose(apply_ball_pose(*o, &pose), &goalie_pose))
            .collect();
        // Append deterministic trail samples (none at rest / in the default).
        append_trail(&mut objects, &pose);
        // Append goalie-volume debug markers over the animated volumes (only
        // when debug is enabled).
        append_goalie_debug(&mut objects, debug, &state.goalie.animated_volumes());

        let mut hud = PenaltyHudModel::from_state(state);
        hud.debug_contact = debug
            .enabled
            .then(|| contact_label(state.contact.map(|f| f.contact_kind())));
        let render_plan =
            PenaltyRenderPlan::build(&objects, &hud, diorama.camera, diorama.style_pass);
        Stage1Diorama { objects, render_plan, hud }
    }

    /// Build the frame for a whole session (Pass 9 + Pass 10): the current
    /// shot's diorama, the dynamic HUD, and — when an impact effect is playing —
    /// the deterministic net wobble / post shake / ball deflection / crowd
    /// reaction / impact flashes and the additive camera juice.
    pub fn build_session_frame(session: &PenaltySessionState) -> Stage1Diorama {
        let diorama = StaticDiorama::stage1();
        let pose = session.shot.ball_pose();
        let goalie_pose = session.shot.goalie.descriptor();

        let mut objects: Vec<DioramaObject> = diorama
            .objects
            .iter()
            .map(|o| apply_goalie_pose(apply_ball_pose(*o, &pose), &goalie_pose))
            .collect();
        append_trail(&mut objects, &pose);

        // Apply the impact-polish effect (deterministic; empty when none).
        let camera = session
            .effect_descriptor()
            .map(|desc| {
                apply_effect(&mut objects, &desc);
                append_effect_items(&mut objects, &desc);
                offset_camera(diorama.camera, desc.camera.offset)
            })
            .unwrap_or(diorama.camera);

        let hud = PenaltyHudModel::from_session(session);
        let render_plan = PenaltyRenderPlan::build(&objects, &hud, camera, diorama.style_pass);
        Stage1Diorama { objects, render_plan, hud }
    }

    /// The default (start) interaction state — centered aim, zero power, ball at
    /// the penalty spot.
    pub fn default_interaction() -> PenaltyInteractionState {
        PenaltyInteractionState::start()
    }

    /// A fresh 5-round session.
    pub fn new_session() -> PenaltySessionState {
        PenaltySessionState::new()
    }
}

/// The neutral debug label for a contact kind (never a final result word).
fn contact_label(kind: Option<PenaltyGoalieContactKind>) -> &'static str {
    match kind.unwrap_or(PenaltyGoalieContactKind::None) {
        PenaltyGoalieContactKind::None => "NONE",
        PenaltyGoalieContactKind::Hand => "HAND",
        PenaltyGoalieContactKind::Torso => "TORSO",
        PenaltyGoalieContactKind::Body => "BODY",
    }
}

/// The debug material for a volume kind (all unlit HUD colors).
fn debug_material(kind: PenaltyGoalieVolumeKind) -> PenaltyMaterialId {
    match kind {
        PenaltyGoalieVolumeKind::LeftHand | PenaltyGoalieVolumeKind::RightHand => {
            PenaltyMaterialId::HudGreenSuccess
        }
        PenaltyGoalieVolumeKind::Torso => PenaltyMaterialId::HudYellowHighlight,
        PenaltyGoalieVolumeKind::Body => PenaltyMaterialId::HudRedWarning,
    }
}

/// Offset the fixed camera by the additive impact-juice offset (the base camera
/// stays authoritative; this is a temporary descriptor).
fn offset_camera(camera: CameraConfig, offset: Vec3) -> CameraConfig {
    CameraConfig { eye: camera.eye.add(offset), target: camera.target.add(offset), ..camera }
}

/// Apply the impact-polish effect descriptor to the object list (deterministic):
/// deflect the ball, shake the hit post, bounce the crowd, and append net-wobble
/// + foreground-flash render items.
fn apply_effect(objects: &mut [DioramaObject], desc: &PenaltyEffectDescriptor) {
    let mut crowd_ordinal = 0u32;
    objects.iter_mut().for_each(|o| {
        if let Some(defl) = desc.ball_deflection {
            (o.label == "ball").then(|| o.position = defl.current);
            (o.label == "shadow.ball")
                .then(|| o.position = Vec3::new(defl.current.x, o.position.y, defl.current.z));
        }
        if let Some(shake) = desc.frame_shake {
            let target_label = match shake.target {
                PenaltyGoalFramePart::LeftPost => "goal.post.left",
                PenaltyGoalFramePart::RightPost => "goal.post.right",
                PenaltyGoalFramePart::Crossbar => "goal.crossbar",
            };
            (o.label == target_label).then(|| o.position = o.position.add(shake.offset));
        }
        (o.label == "crowd.card").then(|| {
            o.position = o.position.add(desc.crowd.card_offset(crowd_ordinal));
            crowd_ordinal += 1;
        });
    });
}

/// Append the net-wobble + foreground-flash render items (kept out of the
/// `&mut [..]` pass above so it can grow the vector).
fn append_effect_items(objects: &mut Vec<DioramaObject>, desc: &PenaltyEffectDescriptor) {
    if let Some(wobble) = &desc.net_wobble {
        append_wobble_nodes(objects, &wobble.rear, DioramaRole::RearNet, "net.wobble.rear");
        append_wobble_nodes(objects, &wobble.front, DioramaRole::FrontNet, "net.wobble.front");
    }
    desc.foreground.iter().for_each(|it| {
        let id = ObjectId(objects.len() as u32);
        objects.push(DioramaObject {
            id,
            role: DioramaRole::ImpactEffect,
            shape: PrimitiveShape::Quad,
            position: it.position,
            size: Vec3::new(it.size, it.size, 0.0),
            material: PenaltyMaterialId::HudYellowHighlight,
            label: it.label,
        });
    });
}

fn append_wobble_nodes(
    objects: &mut Vec<DioramaObject>,
    nodes: &[crate::soccer_penalty::penalty_effects::PenaltyNetWobbleNode],
    role: DioramaRole,
    label: &'static str,
) {
    nodes.iter().for_each(|n| {
        let id = ObjectId(objects.len() as u32);
        objects.push(DioramaObject {
            id,
            role,
            shape: PrimitiveShape::Line,
            position: n.displaced_position,
            size: Vec3::new(0.06, 0.06, 0.06),
            material: PenaltyMaterialId::NetOffWhite,
            label,
        });
    });
}

/// A debug marker's billboard half-size from its volume shape.
fn debug_extent(volume: &PenaltyGoalieVolume) -> (f32, f32) {
    match volume.shape {
        PenaltyGoalieVolumeShape::Sphere { radius } => (radius, radius),
        PenaltyGoalieVolumeShape::Aabb { half_extents } => (half_extents.x, half_extents.y),
    }
}

/// Append one debug quad per goalie volume (empty when debug is disabled), over
/// the given (animated) volume set, with stable sequential ids, in the
/// ForegroundEffects layer.
fn append_goalie_debug(
    objects: &mut Vec<DioramaObject>,
    debug: PenaltyGoalieDebugDescriptor,
    set: &PenaltyGoalieVolumeSet,
) {
    let markers = debug.markers(set);
    let base = objects.len() as u32;
    markers.iter().enumerate().for_each(|(i, v)| {
        let (hx, hy) = debug_extent(v);
        objects.push(DioramaObject {
            id: ObjectId(base + i as u32),
            role: DioramaRole::GoalieDebugVolume,
            shape: PrimitiveShape::Quad,
            position: v.center,
            size: Vec3::new(hx * 2.0, hy * 2.0, 0.0),
            material: debug_material(v.kind),
            label: "goalie.debug.volume",
        });
    });
}

/// Replace the ball and its blob shadow with the current pose (leaves every
/// other object untouched). At rest this reproduces the static positions.
fn apply_ball_pose(o: DioramaObject, pose: &PenaltyBallPose) -> DioramaObject {
    let ball = (o.label == "ball")
        .then_some(DioramaObject {
            position: pose.position,
            size: Vec3::new(pose.radius, pose.radius, pose.radius),
            ..o
        });
    let shadow = (o.label == "shadow.ball").then_some(DioramaObject {
        position: pose.shadow_center,
        size: Vec3::new(pose.shadow_radius_x * 2.0, 0.0, pose.shadow_radius_z * 2.0),
        ..o
    });
    ball.or(shadow).unwrap_or(o)
}

/// Overlay the sampled goalie pose onto a goalie part object (matched by label).
/// Non-goalie objects pass through unchanged; at the idle pose this reproduces
/// the emitted rest positions.
fn apply_goalie_pose(o: DioramaObject, pose: &PenaltyGoaliePoseDescriptor) -> DioramaObject {
    PenaltyGoaliePartKind::from_label(o.label)
        .map(|kind| {
            let part = pose.part(kind);
            DioramaObject { position: part.world.translation, size: part.size, ..o }
        })
        .unwrap_or(o)
}

/// Append the ball trail samples as small quads in the ForegroundEffects layer,
/// with stable sequential ids after the existing objects.
fn append_trail(objects: &mut Vec<DioramaObject>, pose: &PenaltyBallPose) {
    let base = objects.len() as u32;
    (0..pose.trail_len as usize).for_each(|i| {
        // Older samples (larger i) shrink for a fading tail.
        let fade = 1.0 - (i as f32 / (pose.trail_len as f32 + 1.0));
        let s = BALL_RADIUS * 0.8 * fade;
        objects.push(DioramaObject {
            id: ObjectId(base + i as u32),
            role: DioramaRole::BallTrail,
            shape: crate::soccer_penalty::low_poly_assets::PrimitiveShape::Quad,
            position: pose.trail[i],
            size: Vec3::new(s, s, 0.0),
            material: PenaltyMaterialId::BallWhite,
            label: "ball.trail",
        });
    });
}
