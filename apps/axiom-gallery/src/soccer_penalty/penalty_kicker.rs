//! The articulated penalty kicker — driven by the **authored / kinematic** kick pose.
//!
//! The kicker's box geometry is the shared **figure** (`assets/soccer/kicker.figure`)
//! — 13 parts with their sizes, offsets and kit tags — and the *pose* comes from
//! [`crate::soccer_penalty::penalty_kick_pose::SoccerPenaltyKickPose`]: the authored
//! nine-phase [`crate::soccer_penalty::penalty_kick_motion`] motion plan evaluated by
//! pure forward kinematics (`AnimationAuthoringApi::sample` + `frame_joint_world`).
//! Each authored tick yields the 13 joints' world transforms, which pose the figure's
//! boxes (placed at the kicker's spot, `+Z` figure-forward mapped to `-Z`
//! world-forward toward the goal).
//!
//! Reading the pose directly keeps the whole figure coherent. The earlier
//! physics-backed path posed the figure's root box from a free-integrating *dynamic*
//! pelvis body, which drifted/tumbled away from the kinematically-driven limbs — the
//! inverted body + orphan capsule. That path is now behind the
//! `experimental_physical_humanoid_kicker` feature (`penalty_physics_kick`); see
//! `docs/KICK_ANIMATION.md`.
//!
//! The whole kick is aim-independent, so it is simulated once and shared through a
//! process-wide cache; the game samples it by authored tick via [`kicker_frame`].

use std::sync::OnceLock;

use axiom_figure::{FigureApi, FigureDefinition};
use axiom_math::{Quat, Transform, Vec3};

use crate::soccer_penalty::penalty_interaction::PenaltyInteractionState;
use crate::soccer_penalty::penalty_kick_motion::{DURATION, SPRINT_APPROACH, STRIKE_CONTACT_TICK};
use crate::soccer_penalty::penalty_kick_pose::KICKER_JOINTS;
use crate::soccer_penalty::penalty_materials::PenaltyMaterialId;
use crate::soccer_penalty::penalty_scene::{KICKER_X, KICKER_Z};

// The DEFAULT kicker pose source is the authored / kinematic pose
// (`penalty_kick_pose`): every joint transform is pure forward kinematics, so the
// figure stays coherent. The broken physics-backed humanoid (`penalty_physics_kick`)
// is compiled in only behind the `experimental_physical_humanoid_kicker` feature —
// see `docs/KICK_ANIMATION.md` for the invariants it must satisfy before it can
// become the default again.
#[cfg(not(feature = "experimental_physical_humanoid_kicker"))]
use crate::soccer_penalty::penalty_kick_pose::SoccerPenaltyKickPose;
#[cfg(feature = "experimental_physical_humanoid_kicker")]
use crate::soccer_penalty::penalty_physics_kick::PenaltyPhysicsKick;

/// The shared kicker figure geometry, byte-identical to what the lab emits.
const FIGURE_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/soccer/kicker.figure"));

/// Stable, greppable render labels for the 13 kicker parts, in figure order —
/// the same order as [`KICKER_JOINTS`], so joint `i` poses part `i`.
pub const KICKER_LABELS: [&str; 13] = [
    "kicker.pelvis",
    "kicker.chest",
    "kicker.head",
    "kicker.l_thigh",
    "kicker.l_shin",
    "kicker.l_foot",
    "kicker.r_thigh",
    "kicker.r_shin",
    "kicker.r_foot",
    "kicker.l_upper_arm",
    "kicker.l_forearm",
    "kicker.r_upper_arm",
    "kicker.r_forearm",
];

/// The rest/idle authored tick (phase `setup`): the kicker stands ready behind the
/// ball. Used by the rig tests as a stable reference frame.
pub const IDLE_FRAME: f32 = 0.0;

/// The authored tick the static diorama poses the kicker at: late `backswing`, the
/// kicking leg wound back and weight carried onto the planted support leg — a
/// braced, weighted athlete cocked to strike rather than a limp mannequin.
pub const DISPLAY_FRAME: f32 = 44.0;

/// Which pose source the current build wires into the kicker — a greppable marker
/// the tests assert on. `"kinematic"` is the stable default; `"physical_humanoid"`
/// only appears when the experimental feature is enabled.
#[cfg(not(feature = "experimental_physical_humanoid_kicker"))]
pub const KICKER_POSE_SOURCE: &str = "kinematic";
#[cfg(feature = "experimental_physical_humanoid_kicker")]
pub const KICKER_POSE_SOURCE: &str = "physical_humanoid";

/// The process-wide cached default-style kick (aim-independent poses). Default: the
/// authored / kinematic pose. The frame's `joints: [Transform; 13]` field is the same
/// shape for both sources, so [`KickerRig::boxes_at`] is source-agnostic.
#[cfg(not(feature = "experimental_physical_humanoid_kicker"))]
fn cached_kick() -> &'static SoccerPenaltyKickPose {
    static KICK: OnceLock<SoccerPenaltyKickPose> = OnceLock::new();
    KICK.get_or_init(SoccerPenaltyKickPose::default_kick)
}

/// The process-wide cached default-style **physics** kick (experimental).
#[cfg(feature = "experimental_physical_humanoid_kicker")]
fn cached_kick() -> &'static PenaltyPhysicsKick {
    static KICK: OnceLock<PenaltyPhysicsKick> = OnceLock::new();
    KICK.get_or_init(PenaltyPhysicsKick::default_kick)
}

/// One posed kicker box, ready for [`crate::soccer_penalty::penalty_scene`] to
/// emit or for the per-frame overlay to reposition.
#[derive(Debug, Clone, Copy)]
pub struct KickerBox {
    /// World-space box center.
    pub center: Vec3,
    /// World-space box orientation (the bone rotation, mirror-corrected for the
    /// goal-facing Z-flip) so the part renders as an *oriented* capsule/box along
    /// its bone — not an axis-aligned stick at a joint point.
    pub rotation: Quat,
    /// Full box extents.
    pub size: Vec3,
    /// The material this part is drawn with.
    pub material: PenaltyMaterialId,
    /// The part's stable render label.
    pub label: &'static str,
}

/// The kicker rig: the shared figure geometry posed by the physics-backed kick.
#[derive(Debug)]
pub struct KickerRig {
    figure: FigureDefinition,
}

impl KickerRig {
    /// Load the kicker figure geometry (the physics kick is a shared cache).
    pub fn new() -> Self {
        let figure = FigureApi::new().deserialize(FIGURE_BYTES).expect("embedded kicker figure");
        Self { figure }
    }

    /// The kicker's posed boxes at authored `tick`, in world space at the kicker's
    /// spot and facing the goal. The 13 joint world transforms come from the
    /// physics-backed kick (`penalty_physics_kick`); the figure supplies the box
    /// sizes, offsets and kit tags.
    pub fn boxes_at(&self, tick: f32) -> Vec<KickerBox> {
        debug_assert_eq!(self.figure.part_count(), KICKER_JOINTS.len());
        // Interpolate the joint transforms at the (possibly fractional) authored tick,
        // so a slowed run-up plays smoothly between the captured keyframes.
        let world: Vec<Transform> = cached_kick().joints_at(tick).to_vec();
        let posed = FigureApi::new().posed_parts(&self.figure, &world).expect("posed parts");
        posed
            .iter()
            .enumerate()
            .map(|(i, pp)| {
                let p = pp.transform.translation;
                // The world map mirrors the figure across the XY plane (`KICKER_Z -
                // p.z`) to face the goal. Conjugating a rotation by that reflection
                // (`diag(1,1,-1)`) negates its X/Y components and preserves Z/W — so
                // the box renders at the correctly-oriented world rotation.
                let r = pp.transform.rotation;
                KickerBox {
                    center: Vec3::new(KICKER_X + p.x, p.y, KICKER_Z - p.z),
                    rotation: Quat::new(-r.x, -r.y, r.z, r.w),
                    size: pp.box_size,
                    material: material_for_part(i, pp.tag),
                    label: KICKER_LABELS[i],
                }
            })
            .collect()
    }
}

impl Default for KickerRig {
    fn default() -> Self {
        Self::new()
    }
}

/// Map an opaque figure tag to a soccer material.
fn material_for(tag: u32) -> PenaltyMaterialId {
    match tag {
        0 => PenaltyMaterialId::KickerJerseyBlue,
        1 => PenaltyMaterialId::KickerShortsWhite,
        2 => PenaltyMaterialId::KickerSkin,
        _ => PenaltyMaterialId::KickerSocksDark, // socks + boots
    }
}

/// Map a kicker *part* to its material.
///
/// The opaque figure tag alone can't tell three kit surfaces apart: the head
/// (index 2) shares `TAG_SKIN` with the thighs and forearms, and the shins
/// (indices 4, 7) and feet (indices 5, 8) share `TAG_LIMB`/`TAG_END` and both
/// fell to one near-black `KickerSocksDark`. The reference kit, seen from
/// behind, is dark hair on the head, royal-blue socks below the knee, and black
/// boots — so those three parts are resolved by index here; everything else
/// (jersey, shorts, skin) still falls back to the tag-based [`material_for`].
///
/// The **upper arms** (indices 9, 11) are also resolved by index to the jersey
/// blue: the reference #10 wears a short-sleeve kit whose blue sleeve covers the
/// upper arm (bare skin begins at the forearm). This is not only on-model — it is
/// what re-attaches the arms to the body. The athletes bake as one continuous
/// `MetaSurface` **per kit material** (see `body_groups`), so a skin-tagged upper
/// arm baked in the *skin* group could never fuse to the *jersey* torso surface
/// and read as a detached capsule floating at the shoulder. Grouping the upper
/// arm with the jersey torso lets the smooth-union weld the shoulder into one
/// surface, moving the only kit seam down to the elbow — a natural sleeve edge.
fn material_for_part(index: usize, tag: u32) -> PenaltyMaterialId {
    match index {
        2 => PenaltyMaterialId::KickerHair,            // head: dark hair, not bald skin
        4 | 7 => PenaltyMaterialId::KickerSocksBlue,   // shins: royal-blue socks
        5 | 8 => PenaltyMaterialId::KickerShoes,       // feet: black boots
        9 | 11 => PenaltyMaterialId::KickerJerseyBlue, // upper arms: blue sleeve, fuses to torso
        _ => material_for(tag),
    }
}

/// How far before the strike tick the held run-up caps: holding to charge winds the
/// kicker up to the very start of the strike phase (`STRIKE` begins at tick 52) and
/// holds there, cocked, until release fires the strike.
const RUNUP_STRIKE_GAP: f32 = 3.0;

/// Authored ticks advanced per game tick while the run-up is held. **Below 1** so the
/// stride cadence reads as a few deliberate strides rather than a frantic shuffle; the
/// pose is interpolated between keyframes ([`KickerRig::boxes_at`] → `joints_at`) so
/// the sub-tick playback stays smooth instead of stair-stepping. `soccer_shot_state`
/// holds the charge long enough (at this rate) for the run-up to reach the cocked pose.
const RUNUP_PLAYBACK_RATE: f32 = 0.5;

/// The authored kick tick to pose for the current shot state — a **time-based**
/// mapping so the kick actually plays out as an animation (the earlier power-based
/// map crushed the 74-tick kick into the ~8-tick charge, so no run-up ever read):
/// - **Aiming** → the run-up start (`sprint_approach` begin): the kicker stands
///   ready *behind* the ball, not on top of it;
/// - **Charging** → strides forward one authored tick per tick held
///   (`state.charge_ticks`) through `sprint_approach → pre_plant → plant → backswing
///   → hip_drive`, capping cocked just before the strike — a longer hold winds up
///   further, matching "HOLD to charge";
/// - **committed** (locked / in flight / resolved) → `strike … recover`, mapped from
///   the ball's flight progress, so the instep connects as the ball launches and the
///   leg follows through and recovers as the ball flies.
pub fn kicker_frame(state: &PenaltyInteractionState) -> f32 {
    use crate::soccer_penalty::penalty_interaction::PenaltyShotFlightState as S;
    match state.state {
        S::Aiming => SPRINT_APPROACH.0 as f32,
        S::Charging => {
            let start = SPRINT_APPROACH.0 as f32;
            let cocked = STRIKE_CONTACT_TICK as f32 - RUNUP_STRIKE_GAP;
            (start + state.charge_ticks as f32 * RUNUP_PLAYBACK_RATE).min(cocked)
        }
        _ => {
            let progress = state
                .flight
                .map(|f| (f.elapsed_ticks as f32) / (f.total().max(1) as f32))
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let start = STRIKE_CONTACT_TICK as f32;
            let end = (DURATION - 1) as f32;
            start + (end - start) * progress
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soccer_penalty::penalty_input::PenaltyInputIntent;

    #[test]
    fn rig_loads_and_poses_thirteen_boxes_from_authored_pose() {
        let rig = KickerRig::new();
        let boxes = rig.boxes_at(IDLE_FRAME);
        assert_eq!(boxes.len(), 13);
        // All parts sit near the kicker's spot in Z (a metre or two of rig depth).
        assert!(boxes.iter().all(|b| (b.center.z - KICKER_Z).abs() < 3.0));
        assert_eq!(boxes[0].label, "kicker.pelvis");
    }

    #[test]
    fn right_foot_sweeps_toward_the_goal_across_the_kick() {
        let rig = KickerRig::new();
        // Right foot is part index 8; world -Z is toward the goal. It is drawn back
        // in the backswing and sweeps toward the goal by the follow-through.
        let back = rig.boxes_at(41.0)[8].center.z; // backswing
        let follow = rig.boxes_at(62.0)[8].center.z; // follow-through
        assert!(back - follow > 0.2, "foot should move toward goal (-Z): back={back}, follow={follow}");
    }

    #[test]
    fn kick_frame_runs_up_while_held_and_strikes_once_committed() {
        // Aiming: the kicker stands ready behind the ball at the run-up start, never
        // at the strike.
        let aiming = PenaltyInteractionState::start();
        assert!(kicker_frame(&aiming) < STRIKE_CONTACT_TICK as f32);
        // Holding to charge strides the run-up forward by TIME held, but caps cocked
        // just before the strike — charging alone never reaches contact.
        let short = PenaltyInteractionState::run(&[PenaltyInputIntent::charging(0, 0); 6]);
        let long = PenaltyInteractionState::run(&[PenaltyInputIntent::charging(0, 0); 60]);
        assert!(kicker_frame(&long) > kicker_frame(&short), "a longer hold winds up further");
        assert!(kicker_frame(&long) < STRIKE_CONTACT_TICK as f32, "the hold caps cocked, before contact");
        // Release commits the shot; once the ball is in flight the tick is at/after
        // the strike.
        let mut intents = vec![PenaltyInputIntent::charging(0, 0); 20];
        intents.push(PenaltyInputIntent::releasing());
        intents.push(PenaltyInputIntent::neutral()); // LockedPreview → launch
        intents.push(PenaltyInputIntent::neutral()); // BallInFlight
        let launched = PenaltyInteractionState::run(&intents);
        assert!(kicker_frame(&launched) >= STRIKE_CONTACT_TICK as f32);
    }

    #[test]
    fn material_mapping_covers_every_tag() {
        assert_eq!(material_for(0), PenaltyMaterialId::KickerJerseyBlue);
        assert_eq!(material_for(1), PenaltyMaterialId::KickerShortsWhite);
        assert_eq!(material_for(2), PenaltyMaterialId::KickerSkin);
        assert_eq!(material_for(4), PenaltyMaterialId::KickerSocksDark);
    }

    #[test]
    fn head_shins_and_feet_are_resolved_by_part_index() {
        // Head (index 2) is dark hair, not the skin its tag would give.
        assert_eq!(material_for_part(2, 2), PenaltyMaterialId::KickerHair);
        // Shins (4, 7) are royal-blue socks; feet (5, 8) are black boots.
        assert_eq!(material_for_part(4, 3), PenaltyMaterialId::KickerSocksBlue);
        assert_eq!(material_for_part(7, 3), PenaltyMaterialId::KickerSocksBlue);
        assert_eq!(material_for_part(5, 4), PenaltyMaterialId::KickerShoes);
        assert_eq!(material_for_part(8, 4), PenaltyMaterialId::KickerShoes);
        // Upper arms (9, 11) are the blue jersey sleeve, so they bake into the
        // torso's MetaSurface group and fuse at the shoulder (bare-skin tag notwithstanding).
        assert_eq!(material_for_part(9, 2), PenaltyMaterialId::KickerJerseyBlue);
        assert_eq!(material_for_part(11, 2), PenaltyMaterialId::KickerJerseyBlue);
        // Everything else falls back to the tag: jersey, shorts, skin (thigh/forearm).
        assert_eq!(material_for_part(1, 0), PenaltyMaterialId::KickerJerseyBlue);
        assert_eq!(material_for_part(0, 1), PenaltyMaterialId::KickerShortsWhite);
        assert_eq!(material_for_part(3, 2), PenaltyMaterialId::KickerSkin);
        assert_eq!(material_for_part(10, 2), PenaltyMaterialId::KickerSkin);
    }

    #[test]
    fn head_reads_as_hair_and_hands_stay_skin_in_world() {
        let boxes = KickerRig::new().boxes_at(IDLE_FRAME);
        assert_eq!(boxes[2].label, "kicker.head");
        assert_eq!(boxes[2].material, PenaltyMaterialId::KickerHair);
        // Forearm/hand tips keep skin so only the head turns to hair.
        assert_eq!(boxes[10].material, PenaltyMaterialId::KickerSkin);
        assert_eq!(boxes[12].material, PenaltyMaterialId::KickerSkin);
    }
}
