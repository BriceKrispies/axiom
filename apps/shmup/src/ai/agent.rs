//! Ported from Claude-of-Duty `src/ai/agent.js:1-1009` — the whole file's
//! logic.
//!
//! One enemy: body, senses, brain, gun.
//!
//! PERCEPTION is deliberately imperfect. A target has to be inside a 100 degree
//! cone, in line of sight through the physics BVH, and then *stay* there for a
//! reaction delay that scales with angle off-centre and distance before the
//! agent acknowledges it. Gunshots and footsteps arrive as events and only give
//! a direction, which becomes a "last known position" that decays — so enemies
//! search where you were, not where you are.
//!
//! BEHAVIOUR is a small state machine:
//!   idle / patrol -> alert -> combat -> suppressed -> flank -> retreat -> dead
//!
//! DAMAGE is per-bone: capsule colliders for head, chest, pelvis, arms and legs
//! are pushed onto the animated skeleton every frame, so a headshot is a
//! headshot because of where the round landed.
//!
//! ## What is *not* here, and why — the honest boundary
//!
//! An earlier pass of this port stopped at ~74% and justified the gap in prose.
//! Re-audited (`docs/work-manifests/shmup-port/notes/ai-agent.md`), most of
//! that prose was an unfinished port wearing a justification: `_shoot`'s burst
//! and ammunition logic, `_fireRound`'s spread draws, `applyDamage`'s
//! hit-region selection, `die`'s impulse, `_drive`'s clip selection and
//! animation-rate LOD, `_tryVault`, the character-controller integration in
//! `_move`, and the whole `update` tick driver are all pure logic and are now
//! ported. Three exclusions survive the audit as genuine engine-arm
//! boundaries, and each is reduced to the *narrowest trait naming exactly the
//! call the source makes* — the precedent [`super::grounding::FootSource`]
//! already set:
//!
//! 1. **The skinned body and the scene graph** — `new THREE.SkinnedMesh(...)`,
//!    `group.add`/`updateMatrixWorld`, `mesh.bind(skeleton)`
//!    (`agent.js:104-132`). Pure rendering; the engine's render arm is a
//!    separate slice. Nothing here creates a mesh.
//! 2. **The animator's pose evaluation** — `animator.js` is 559 lines of
//!    layered blending and four IK solvers, listed as its own row in
//!    `06-parallel-port-plan.md`. `agent.js` *calls into* it, and every one of
//!    those calls is behaviour, so they are ported against
//!    [`AgentAnimator`], which names the eleven members `agent.js` actually
//!    touches (`reloading`, `vaulting`, `muzzleWorld`, `muzzleDir`, `bonePos`,
//!    `fire`, `hit`, `reload`, `vault`, `turn`, `setState`, `update`,
//!    `enabled`).
//! 3. **The ragdoll solver hand-off** — `phys.createRagdollFromSkeleton`
//!    (`agent.js:901-909`) enters `physics/ragdoll.js`, 763 lines of PBD
//!    listed as unported. [`Agent::die`] ports everything around it (the
//!    impulse, the hit point, the collider/controller teardown, the
//!    `actor:death` event) and stops at the solver call itself; the 15 cm
//!    lift/unlift either side of it (`agent.js:898-911`) is a workaround for
//!    that solver's contact behaviour and is documented at the site rather
//!    than faked.
//!
//! Everything else the constructor builds but never reads (`searchPoint`,
//! `reactionTimer`, `aimActual`, the `DOLL` table, `DEG`) is dead in the
//! source and is carried here with a comment, per the recipe's "dead
//! computation in the source is still part of the source".
//!
//! The module-global `_nextId` counter (`agent.js:90,96`) is a hidden mutable
//! static; [`next_agent_id`] makes the counter an explicit argument instead —
//! the same choice already made for [`super::squad::Squad::new`]'s `id`.

use crate::rng::Rng;

use super::nav::{self, CoverPoint, SquadMemberPos, WorldProbe};
use super::squad::{ContactBroadcast, MemberSnapshot, Squad};
use crate::jsmath;
use crate::physics::surfaces::{layer, mask};
use crate::world::palette::Surface;

/// `const DEG = Math.PI / 180;` (`agent.js:88`). Declared in the source and
/// never used by it — carried per the recipe's "dead computation in the
/// source is still part of the source".
pub const DEG: f64 = std::f64::consts::PI / 180.0;

/// `_nextId++` (`agent.js:90,96`), as an explicit counter rather than a
/// module-global mutable static. Start the counter at `1`, as the source does.
pub fn next_agent_id(counter: &mut i32) -> i32 {
    let id = *counter;
    *counter += 1;
    id
}

/// `STATE`. `agent.js:27-38`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentState {
    #[default]
    Idle,
    Patrol,
    Alert,
    Combat,
    Suppressed,
    Flank,
    Retreat,
    Dead,
}

impl AgentState {
    /// The source's string value, so a golden captured from the original can
    /// be compared without a hand-written mapping table.
    pub fn as_str(self) -> &'static str {
        match self {
            AgentState::Idle => "idle",
            AgentState::Patrol => "patrol",
            AgentState::Alert => "alert",
            AgentState::Combat => "combat",
            AgentState::Suppressed => "suppressed",
            AgentState::Flank => "flank",
            AgentState::Retreat => "retreat",
            AgentState::Dead => "dead",
        }
    }
}

/// The damage region a hitbox capsule belongs to — the `part` string a
/// collider carries (`agent.js:40-48,165`) and `applyDamage` switches on
/// (`agent.js:830-834`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyPart {
    Head,
    Torso,
    Arm,
    Leg,
}

impl BodyPart {
    pub fn as_str(self) -> &'static str {
        match self {
            BodyPart::Head => "head",
            BodyPart::Torso => "torso",
            BodyPart::Arm => "arm",
            BodyPart::Leg => "leg",
        }
    }
}

/// The region name `applyDamage` hands `animator.hit`
/// (`agent.js:830-835`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitRegion {
    Head,
    Torso,
    ArmR,
    ArmL,
    LegR,
    LegL,
}

impl HitRegion {
    pub fn as_str(self) -> &'static str {
        match self {
            HitRegion::Head => "head",
            HitRegion::Torso => "torso",
            HitRegion::ArmR => "armR",
            HitRegion::ArmL => "armL",
            HitRegion::LegR => "legR",
            HitRegion::LegL => "legL",
        }
    }
}

/// The locomotion clip `_drive` selects (`agent.js:946-951`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clip {
    #[default]
    Idle,
    Walk,
    Run,
    CrouchWalk,
    CrouchIdle,
    HurtIdle,
}

impl Clip {
    pub fn as_str(self) -> &'static str {
        match self {
            Clip::Idle => "idle",
            Clip::Walk => "walk",
            Clip::Run => "run",
            Clip::CrouchWalk => "crouchWalk",
            Clip::CrouchIdle => "crouchIdle",
            Clip::HurtIdle => "hurtIdle",
        }
    }
}

/// One row of `HITBOXES` (`agent.js:40-48`):
/// `[part, boneA, boneB, radius, damageScale]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitboxSpec {
    pub part: BodyPart,
    pub a: &'static str,
    pub b: &'static str,
    /// Multiplied by the agent's `scale` when the collider is created.
    pub radius: f64,
    pub damage_scale: f64,
}

/// `HITBOXES`. `agent.js:40-48`.
pub const HITBOXES: [HitboxSpec; 7] = [
    HitboxSpec { part: BodyPart::Head, a: "Head", b: "HeadTop", radius: 0.098, damage_scale: 4.0 },
    HitboxSpec { part: BodyPart::Torso, a: "Spine1", b: "Neck", radius: 0.185, damage_scale: 1.0 },
    HitboxSpec { part: BodyPart::Torso, a: "Hips", b: "Spine1", radius: 0.175, damage_scale: 0.9 },
    HitboxSpec { part: BodyPart::Arm, a: "UpperArmR", b: "HandR", radius: 0.072, damage_scale: 0.65 },
    HitboxSpec { part: BodyPart::Arm, a: "UpperArmL", b: "HandL", radius: 0.072, damage_scale: 0.65 },
    HitboxSpec { part: BodyPart::Leg, a: "UpLegR", b: "FootR", radius: 0.105, damage_scale: 0.7 },
    HitboxSpec { part: BodyPart::Leg, a: "UpLegL", b: "FootL", radius: 0.105, damage_scale: 0.7 },
];

/// One row of `DOLL` (`agent.js:60-86`):
/// `[headBone, tailBone, radius, massFraction, parentIndex, cone°, twist°, map]`.
///
/// `map == false` marks a stub whose only job is to weld a limb chain to the
/// torso: the solver shares a particle between two bones only when their
/// endpoints are coincident, so the shoulder and hip need a bone that starts
/// exactly on the spine joint.
///
/// **Dead in the source.** `agent.js` declares `DOLL` and never references it
/// again — `_makeRagdoll` calls `phys.createRagdollFromSkeleton`, which
/// derives its own chain. Carried here per the recipe's "dead computation in
/// the source is still part of the source"; the judgement that it is dead can
/// be wrong, and preserving it costs nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DollBone {
    pub head: &'static str,
    pub tail: &'static str,
    pub radius: f64,
    pub mass_fraction: f64,
    pub parent: i32,
    pub cone_deg: f64,
    pub twist_deg: f64,
    pub map: bool,
}

/// `DOLL`. `agent.js:60-86`. See [`DollBone`] — dead in the source.
pub const DOLL: [DollBone; 22] = [
    DollBone { head: "Hips", tail: "Spine", radius: 0.135, mass_fraction: 0.14, parent: -1, cone_deg: 0.0, twist_deg: 0.0, map: true },
    DollBone { head: "Spine", tail: "Spine1", radius: 0.125, mass_fraction: 0.10, parent: 0, cone_deg: 22.0, twist_deg: 16.0, map: true },
    DollBone { head: "Spine1", tail: "Spine2", radius: 0.135, mass_fraction: 0.14, parent: 1, cone_deg: 18.0, twist_deg: 12.0, map: true },
    DollBone { head: "Spine2", tail: "Neck", radius: 0.130, mass_fraction: 0.10, parent: 2, cone_deg: 16.0, twist_deg: 10.0, map: true },
    DollBone { head: "Neck", tail: "Head", radius: 0.052, mass_fraction: 0.03, parent: 3, cone_deg: 30.0, twist_deg: 25.0, map: true },
    DollBone { head: "Head", tail: "HeadTop", radius: 0.098, mass_fraction: 0.07, parent: 4, cone_deg: 42.0, twist_deg: 30.0, map: true },
    // stubs get a free cone: their direction is lateral while the parent points
    // up the spine, so any limit here is violated in the bind pose and the
    // solver would inject energy trying to fix it
    DollBone { head: "Spine2", tail: "UpperArmR", radius: 0.055, mass_fraction: 0.02, parent: 3, cone_deg: 179.0, twist_deg: 179.0, map: false },
    DollBone { head: "UpperArmR", tail: "ForearmR", radius: 0.058, mass_fraction: 0.027, parent: 6, cone_deg: 100.0, twist_deg: 60.0, map: true },
    DollBone { head: "ForearmR", tail: "HandR", radius: 0.048, mass_fraction: 0.018, parent: 7, cone_deg: 80.0, twist_deg: 45.0, map: true },
    DollBone { head: "HandR", tail: "FingersR", radius: 0.038, mass_fraction: 0.006, parent: 8, cone_deg: 55.0, twist_deg: 40.0, map: true },
    DollBone { head: "Spine2", tail: "UpperArmL", radius: 0.055, mass_fraction: 0.02, parent: 3, cone_deg: 179.0, twist_deg: 179.0, map: false },
    DollBone { head: "UpperArmL", tail: "ForearmL", radius: 0.058, mass_fraction: 0.027, parent: 10, cone_deg: 100.0, twist_deg: 60.0, map: true },
    DollBone { head: "ForearmL", tail: "HandL", radius: 0.048, mass_fraction: 0.018, parent: 11, cone_deg: 80.0, twist_deg: 45.0, map: true },
    DollBone { head: "HandL", tail: "FingersL", radius: 0.038, mass_fraction: 0.006, parent: 12, cone_deg: 55.0, twist_deg: 40.0, map: true },
    DollBone { head: "Hips", tail: "UpLegR", radius: 0.065, mass_fraction: 0.02, parent: 0, cone_deg: 179.0, twist_deg: 179.0, map: false },
    DollBone { head: "UpLegR", tail: "LegR", radius: 0.088, mass_fraction: 0.10, parent: 14, cone_deg: 95.0, twist_deg: 35.0, map: true },
    DollBone { head: "LegR", tail: "FootR", radius: 0.068, mass_fraction: 0.045, parent: 15, cone_deg: 70.0, twist_deg: 20.0, map: true },
    DollBone { head: "FootR", tail: "ToeR", radius: 0.050, mass_fraction: 0.012, parent: 16, cone_deg: 40.0, twist_deg: 20.0, map: true },
    DollBone { head: "Hips", tail: "UpLegL", radius: 0.065, mass_fraction: 0.02, parent: 0, cone_deg: 179.0, twist_deg: 179.0, map: false },
    DollBone { head: "UpLegL", tail: "LegL", radius: 0.088, mass_fraction: 0.10, parent: 18, cone_deg: 95.0, twist_deg: 35.0, map: true },
    DollBone { head: "LegL", tail: "FootL", radius: 0.068, mass_fraction: 0.045, parent: 19, cone_deg: 70.0, twist_deg: 20.0, map: true },
    DollBone { head: "FootL", tail: "ToeL", radius: 0.050, mass_fraction: 0.012, parent: 20, cone_deg: 40.0, twist_deg: 20.0, map: true },
];

/* ==================================================================== */
/* Seams: the narrowest trait per unported collaborator                  */
/* ==================================================================== */

/// `ai.requestPath(from, dest, out)` (`agent.js:591`) — the AI system's
/// per-frame-budgeted A* front end. `None` is the source's `n < 0` ("the
/// frame's A* budget is spent"); `Some(vec![])` is its `n === 0` ("no route").
///
/// [`nav::NavGrid`] implements this with no budget (it always answers), which
/// is what `ai/index.js` degrades to once `pathsPerFrame` is not exhausted.
pub trait PathSource {
    fn request_path(&mut self, from: [f64; 3], to: [f64; 3]) -> Option<Vec<[f64; 3]>>;
}

impl PathSource for nav::NavGrid {
    fn request_path(&mut self, from: [f64; 3], to: [f64; 3]) -> Option<Vec<[f64; 3]>> {
        Some(self.find_path(from, to, nav::FindPathOpts::default()))
    }
}

/// The cover point `agent.js` holds in `this.cover` — the five fields it reads
/// off it (`pick.x/y/z` at `agent.js:489`, `this.cover.high` at
/// `agent.js:534`), plus the identity it compares with `pick !== this.cover`
/// (`agent.js:487`), which this port carries as the point's index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverPick {
    pub index: usize,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub high: bool,
}

/// The `opts` object `_combat` builds for `cover.pick` (`agent.js:479-485`).
#[derive(Debug, Clone, Copy)]
pub struct CoverRequest<'a> {
    pub id: i32,
    pub squad: Option<&'a [SquadMemberPos]>,
    pub min_range: f64,
    pub max_range: f64,
    pub max_travel: f64,
}

/// `ai.cover` (`nav.js`'s `CoverMap`), narrowed to the three calls `agent.js`
/// makes on it: `pick` (`agent.js:479`), `peekOffset` (`agent.js:530`) and
/// `release` (`agent.js:507,601,852`).
///
/// [`nav::CoverMap`] implements it, so the real map wires straight in; a test
/// can script one instead.
pub trait CoverSource {
    fn pick(&mut self, pos: [f64; 3], threat: [f64; 3], opts: CoverRequest) -> Option<CoverPick>;
    fn peek_offset(
        &self,
        cover: &CoverPick,
        threat: [f64; 3],
        eye_h: f64,
        phys: &dyn WorldProbe,
    ) -> (i32, [f64; 3]);
    fn release(&mut self, claim_id: i32);
}

impl CoverSource for nav::CoverMap {
    fn pick(&mut self, pos: [f64; 3], threat: [f64; 3], opts: CoverRequest) -> Option<CoverPick> {
        let idx = nav::CoverMap::pick(
            self,
            pos,
            threat,
            nav::PickOpts {
                min_range: opts.min_range,
                max_range: opts.max_range,
                id: opts.id,
                squad: opts.squad,
                max_travel: opts.max_travel,
                ..nav::PickOpts::default()
            },
        )?;
        let p = self.points[idx];
        Some(CoverPick { index: idx, x: p.x, y: p.y, z: p.z, high: p.high })
    }

    fn peek_offset(
        &self,
        cover: &CoverPick,
        threat: [f64; 3],
        eye_h: f64,
        phys: &dyn WorldProbe,
    ) -> (i32, [f64; 3]) {
        let p: CoverPoint = self.points[cover.index];
        nav::CoverMap::peek_offset(self, &p, threat, eye_h, phys)
    }

    fn release(&mut self, claim_id: i32) {
        nav::CoverMap::release(self, claim_id);
    }
}

/// `this.squad` (`squad.js`'s `Squad`), narrowed to the four permission calls
/// `agent.js` makes on it: `requestPeek` (`agent.js:526`), `canFlank`
/// (`agent.js:548`), `claimFlank` (`agent.js:562`) and `requestGrenade`
/// (`agent.js:574`).
///
/// The port keeps this a trait rather than a `&mut Squad` because
/// [`super::squad::Squad`] answers `canFlank` from a `&[MemberSnapshot]` read
/// view the agent has no business owning — [`SquadSeat`] pairs the two.
pub trait SquadPermissions {
    /// `sq.requestPeek(this, dt)`. The source's `dt` argument is unused by
    /// `squad.js:82-87`.
    fn request_peek(&mut self, agent_id: i32, dt: f64) -> bool;
    fn can_flank(&mut self, agent_id: i32) -> bool;
    fn claim_flank(&mut self, agent_id: i32);
    fn request_grenade(&mut self) -> bool;
}

/// A [`super::squad::Squad`] plus the frame's member snapshots — the pair
/// [`SquadPermissions`] needs, since `canFlank` reads other members' live
/// state and the squad itself stores only ids.
pub struct SquadSeat<'a> {
    pub squad: &'a mut Squad,
    pub members: &'a [MemberSnapshot],
}

impl SquadPermissions for SquadSeat<'_> {
    fn request_peek(&mut self, agent_id: i32, _dt: f64) -> bool {
        self.squad.request_peek(agent_id)
    }
    fn can_flank(&mut self, agent_id: i32) -> bool {
        self.squad.can_flank(agent_id, self.members)
    }
    fn claim_flank(&mut self, agent_id: i32) {
        self.squad.claim_flank(agent_id);
    }
    fn request_grenade(&mut self) -> bool {
        self.squad.request_grenade()
    }
}

/// The state block `_drive` hands `animator.setState` (`agent.js:954-962`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimatorState {
    pub clip: Clip,
    pub speed: f64,
    pub crouch: bool,
    pub aim_target: [f64; 3],
    pub look_target: [f64; 3],
    pub aim_weight: f64,
    pub suppress: f64,
}

/// `this.animator` (`animator.js`'s `Animator`), narrowed to exactly what
/// `agent.js` touches. See the module doc comment for why the animator itself
/// is a separate slice.
pub trait AgentAnimator {
    /// `an.reloading` (`animator.js:204`, `reloadT >= 0`).
    fn reloading(&self) -> bool;
    /// `an.vaulting` (`animator.js:213`, `vaultT >= 0`).
    fn vaulting(&self) -> bool;
    /// `an.muzzleWorld`.
    fn muzzle_world(&self) -> [f64; 3];
    /// `an.muzzleDir`.
    fn muzzle_dir(&self) -> [f64; 3];
    /// `an.bonePos(name, out)`.
    fn bone_pos(&self, bone: &str) -> [f64; 3];
    /// `an.fire(strength)`.
    fn fire(&mut self, strength: f64);
    /// `an.hit(region, side, strength)`.
    fn hit(&mut self, region: HitRegion, side: f64, strength: f64);
    /// `an.reload(duration)`.
    fn reload(&mut self, duration: f64);
    /// `an.vault(duration)`.
    fn vault(&mut self, duration: f64);
    /// `an.turn(dir)`.
    fn turn(&mut self, dir: f64);
    /// `an.setState(s)`.
    fn set_state(&mut self, s: AnimatorState);
    /// `an.update(dt, elapsed)`.
    fn update(&mut self, dt: f64, elapsed: f64);
    /// `an.enabled = v` (`agent.js:851`).
    fn set_enabled(&mut self, enabled: bool);
}

/// `this.controller` — the swept character controller `phys.createCharacter`
/// returns (`agent.js:146-153`), narrowed to the six members `_move`/`_drive`
/// touch. Mirrors the precedent of `crate::player::movement::CharacterController`
/// for the player's own controller.
pub trait AgentController {
    /// `c.position`.
    fn position(&self) -> [f64; 3];
    /// `c.grounded`.
    fn grounded(&self) -> bool;
    /// `c.lastMoveBlocked`.
    fn last_move_blocked(&self) -> bool;
    /// `c.setHeight?.(h)` — optional in the source; a no-op default here.
    fn set_height(&mut self, _h: f64) {}
    /// `c.move(dx, dy, dz)`.
    fn move_by(&mut self, dx: f64, dy: f64, dz: f64);
    /// `c.teleport(x, y, z)`.
    fn teleport_to(&mut self, x: f64, y: f64, z: f64);
}

/// `ai.groundAt(x, z, fromY)` (`agent.js:721`) — the one AI-system query
/// `_tryVault` makes. Returns a non-finite value when no floor was found,
/// matching the source's `Number.isFinite(y)` guard.
pub trait GroundHeight {
    fn ground_at(&self, x: f64, z: f64, from_y: f64) -> f64;
}

/// Another agent's position/radius/liveness, the only fields `_move`'s local
/// avoidance loop reads off a squadmate. `agent.js:632-649`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Neighbor {
    pub id: i32,
    pub alive: bool,
    pub position: [f64; 3],
    pub radius: f64,
}

/// One hitbox collider as `constructor` creates it (`agent.js:160-172`).
/// The engine has no collider registry in this slice, so the agent produces
/// the specs and the caller registers them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColliderSpec {
    pub part: BodyPart,
    pub layer: u16,
    pub surface: Surface,
    /// `r * this.scale`.
    pub radius: f64,
    pub damage_scale: f64,
    /// `c.userData = { a, b }`.
    pub a: &'static str,
    pub b: &'static str,
}

/// One `c.setSegment(...)` call from `syncHitboxes` (`agent.js:995-998`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitboxSegment {
    pub part: BodyPart,
    pub a: [f64; 3],
    pub b: [f64; 3],
}

/// Everything `agent.js` pushes out through the AI system or the event bus,
/// returned as data rather than mutated through a borrowed reference — the
/// same divergence [`super::squad::SquadUpdate`] already makes, and for the
/// same ownership reason.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentEvent {
    /// `this.ai.emitReload(this)` (`agent.js:759`).
    Reload,
    /// `this.ai.onAgentFire(this, origin, dir)` (`agent.js:786`).
    Fire { origin: [f64; 3], dir: [f64; 3] },
    /// `this.ai.throwGrenade(this, from, target)` (`agent.js:793`).
    Grenade { from: [f64; 3], target: [f64; 3] },
    /// `this.ctx.events.emit('actor:death', {...})` (`agent.js:876-881`).
    Death { point: [f64; 3], impulse: [f64; 3], headshot: bool },
}

/// [`Agent::move_step`]'s result: the steering direction (already normalised
/// when non-zero, matching `this._steer` post-normalisation) and the eased
/// speed for this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveStep {
    pub steer: [f64; 3],
    pub speed: f64,
}

/// The per-frame collaborators `update` needs. Each field is one object
/// `agent.js` reaches through `this.ai`, `this.ctx`, `this.phys`,
/// `this.animator` or `this.controller`.
pub struct AgentCtx<'a> {
    /// `ai.playerPosition(out)` — `None` when there is no player.
    pub player: Option<[f64; 3]>,
    /// `this.phys` — `null` in the source when the physics subsystem is absent.
    pub phys: Option<&'a dyn WorldProbe>,
    /// `this.phys.gravity` (`agent.js:677`).
    pub gravity: f64,
    /// `this.ctx.time.elapsed`.
    pub elapsed: f64,
    /// `this.ai.agents` (`agent.js:632`).
    pub neighbors: &'a [Neighbor],
    pub animator: &'a mut dyn AgentAnimator,
    /// `this.controller` — `None` once [`Agent::die`] has cleared it, or when
    /// there is no physics.
    pub controller: Option<&'a mut dyn AgentController>,
    /// `this.ai.grid` / `this.ai.requestPath` — `None` is the source's
    /// `if (!grid)` fast path (`agent.js:585-590`).
    pub path: Option<&'a mut dyn PathSource>,
    /// `this.ai.cover` — `None` mirrors the source's `this.ai.cover?.`.
    pub cover: Option<&'a mut dyn CoverSource>,
    /// `this.squad` — `None` for a lone agent.
    pub squad: Option<&'a mut dyn SquadPermissions>,
    /// `sq?.members` (`agent.js:481`) — the *squad's* members, which is what
    /// `cover.pick`'s bunching penalty receives, not `ai.agents`. `None` when
    /// there is no squad, matching the source's `undefined`.
    pub squad_positions: Option<&'a [SquadMemberPos]>,
    /// `this.ai.groundAt`.
    pub ground: &'a dyn GroundHeight,
}

/// One enemy. `class Agent`, `agent.js:92-1009`.
pub struct Agent {
    pub id: i32,
    pub rng: Rng,
    pub variant_name: String,
    pub scale: f64,

    /* ---------------- body ---------------- */
    /// `82 * this.scale` (`agent.js:123`) — read by `_makeRagdoll`.
    pub mass: f64,
    /// `1.78 * this.scale` (`agent.js:144`).
    pub height: f64,
    /// `0.34 * this.scale` (`agent.js:145`).
    pub radius: f64,
    pub position: [f64; 3],
    pub yaw: f64,
    pub target_yaw: f64,
    pub velocity: [f64; 3],
    pub grounded: bool,
    /// `this.colliders` (`agent.js:158-173`), emptied by `die`/`dispose`.
    pub colliders: Vec<ColliderSpec>,
    /// The agent's own record of whether it still owns a character
    /// controller. The controller itself lives outside the agent here (it is
    /// `AgentCtx::controller`, a trait object the caller owns), so this is the
    /// half of `this.controller` the *agent* is responsible for: `die` clears
    /// it, mirroring `this.controller = null` (`agent.js:854`). Whether one is
    /// bound at all is `AgentCtx::controller.is_some()` — the source's
    /// `phys ? phys.createCharacter(...) : null`.
    pub has_controller: bool,

    /* ---------------- stats ---------------- */
    pub health: f64,
    pub max_health: f64,
    pub alive: bool,
    pub state: AgentState,
    pub state_time: f64,
    pub team: i32,
    /// `this.deadTime`, set to `0` by `die` and driven by `ai/index.js`.
    pub dead_time: Option<f64>,

    /* ---------------- perception ---------------- */
    /// `RIG.eyeHeight * this.scale` (`agent.js:185`).
    pub eye_height: f64,
    pub view_range: f64,
    pub view_cos: f64,
    /// 0..1 build-up before the target is acknowledged.
    pub awareness: f64,
    pub has_target: bool,
    pub target_visible: bool,
    /// `this.target` (`agent.js:191,321`) — the acknowledged player position.
    pub target: Option<[f64; 3]>,
    pub last_known: [f64; 3],
    pub last_known_age: f64,
    /// `this.searchPoint` (`agent.js:194`) — declared by the constructor and
    /// never read or written again. Dead in the source; carried per the
    /// recipe.
    pub search_point: [f64; 3],
    pub suppression: f64,
    /// `this.reactionTimer` (`agent.js:196`) — dead in the source; see
    /// [`Agent::search_point`].
    pub reaction_timer: f64,
    pub alertness: f64,

    /* ---------------- combat ---------------- */
    pub weapon_range: f64,
    pub fire_rate: f64,
    pub burst_left: i32,
    pub fire_cooldown: f64,
    pub burst_cooldown: f64,
    pub mag_size: i32,
    pub ammo: i32,
    pub spread: f64,
    pub weapon_damage: f64,
    pub aim_target: [f64; 3],
    /// `this.aimActual` (`agent.js:210`) — dead in the source; see
    /// [`Agent::search_point`].
    pub aim_actual: [f64; 3],
    pub aim_weight: f64,
    pub want_fire: bool,
    pub peek_side: i32,
    pub peeking: bool,
    pub peek_timer: f64,
    pub grenade_cooldown: f64,
    pub has_grenade: bool,

    /* ---------------- navigation ---------------- */
    pub path: Vec<[f64; 3]>,
    /// `this.pathLen` — the source's `path` array is a reused pool, so the
    /// live prefix length is separate from the array's capacity.
    pub path_len: usize,
    pub path_index: usize,
    pub repath_timer: f64,
    pub move_target: [f64; 3],
    pub has_move_target: bool,
    pub desired_speed: f64,
    pub speed: f64,
    pub crouch: bool,
    pub cover: Option<CoverPick>,
    pub cover_pos: [f64; 3],
    pub patrol_points: Option<Vec<[f64; 3]>>,
    pub patrol_index: usize,
    pub stuck_timer: f64,
    pub vault_cooldown: f64,
    /// A path request the frame budget pushed to the next frame.
    pub path_pending: bool,
    pending_dest: [f64; 3],

    /* ---------------- vault root motion ---------------- */
    /// `this.vaultT` — `undefined` until the first vault (`agent.js:727,933`).
    pub vault_t: Option<f64>,
    pub vault_from: Option<[f64; 3]>,
    pub vault_to: Option<[f64; 3]>,

    /* ---------------- LOD ---------------- */
    /// Set by `AiSystem._updateRelevance`: nothing this actor does reaches a
    /// pixel.
    pub lod_irrelevant: bool,
    anim_skip: i32,
    anim_accum: f64,

    pub clip: Clip,
}

impl Agent {
    /// `constructor(ai, opts)`. `agent.js:93-257`, minus the scene-graph and
    /// skinned-mesh construction (see the module doc comment).
    ///
    /// `rng` must already be `ai.rng.fork()` (`agent.js:97`). The constructor
    /// then makes, **in this order**: one `fork()` for the animator
    /// (`agent.js:136`), `range(0.4, 1.4)` for `burstCooldown`
    /// (`agent.js:204`), `range(0.5, 2.5)` for `peekTimer` (`agent.js:215`)
    /// and `range(9, 22)` for `grenadeCooldown` (`agent.js:216`). The
    /// animator's fork is taken here even though the animator is a later
    /// slice — dropping it would shift every subsequent draw.
    ///
    /// `has_physics` is the source's `const phys = this.ctx.peek('physics')`
    /// (`agent.js:142`). It is a constructor input rather than something the
    /// agent infers, because it gates **two** fields at once and a bodyless
    /// agent is a real configuration the source supports: with no physics
    /// subsystem, `this.controller` is `null` (`agent.js:146-154`) *and*
    /// `this.colliders` stays empty (`agent.js:158-173` — the whole hitbox
    /// loop is inside `if (phys)`, since a collider only exists by being
    /// registered with `phys.addCollider`).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: i32,
        mut rng: Rng,
        variant_name: &str,
        scale: f64,
        rig_eye_height: f64,
        position: [f64; 3],
        yaw: f64,
        has_physics: bool,
    ) -> (Self, Rng) {
        // `agent.js:136` — `new Animator(RIG, bones, { rng: this.rng.fork() })`.
        let animator_rng = rng.fork();
        let burst_cooldown = rng.range(0.4, 1.4);
        let peek_timer = rng.range(0.5, 2.5);
        let grenade_cooldown = rng.range(9.0, 22.0);

        let radius = 0.34 * scale;
        let agent = Agent {
            id,
            rng,
            variant_name: variant_name.to_string(),
            scale,
            mass: 82.0 * scale,
            height: 1.78 * scale,
            radius,
            position,
            yaw,
            target_yaw: yaw,
            velocity: [0.0, 0.0, 0.0],
            grounded: true,
            // `this.colliders = []; if (phys) { for (... of HITBOXES) ... }`
            // — with no physics there is nothing to register a capsule with,
            // so the list stays empty.
            colliders: has_physics
                .then(|| {
                    HITBOXES
                        .iter()
                        .map(|h| ColliderSpec {
                            part: h.part,
                            layer: layer::ACTOR,
                            surface: Surface::Flesh,
                            radius: h.radius * scale,
                            damage_scale: h.damage_scale,
                            a: h.a,
                            b: h.b,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            // `this.controller = phys ? phys.createCharacter(...) : null`
            has_controller: has_physics,
            health: 100.0,
            max_health: 100.0,
            alive: true,
            state: AgentState::Idle,
            state_time: 0.0,
            team: 1,
            dead_time: None,
            eye_height: rig_eye_height * scale,
            view_range: 58.0,
            view_cos: ((100.0f64 * std::f64::consts::PI) / 180.0 / 2.0).cos(),
            awareness: 0.0,
            has_target: false,
            target_visible: false,
            target: None,
            last_known: [0.0, 0.0, 0.0],
            last_known_age: f64::INFINITY,
            search_point: [0.0, 0.0, 0.0],
            suppression: 0.0,
            reaction_timer: 0.0,
            alertness: 0.0,
            weapon_range: 60.0,
            // `agent.js:201`
            fire_rate: if variant_name == "irregular" { 8.2 } else { 10.5 },
            burst_left: 0,
            fire_cooldown: 0.0,
            burst_cooldown,
            mag_size: 30,
            ammo: 30,
            spread: 0.032,
            weapon_damage: 17.0,
            aim_target: [0.0, 0.0, 0.0],
            aim_actual: [0.0, 0.0, 0.0],
            aim_weight: 0.0,
            want_fire: false,
            peek_side: 0,
            peeking: false,
            peek_timer,
            grenade_cooldown,
            has_grenade: true,
            path: Vec::new(),
            path_len: 0,
            path_index: 0,
            repath_timer: 0.0,
            move_target: position,
            has_move_target: false,
            desired_speed: 0.0,
            speed: 0.0,
            crouch: false,
            cover: None,
            cover_pos: [0.0, 0.0, 0.0],
            patrol_points: None,
            patrol_index: 0,
            stuck_timer: 0.0,
            vault_cooldown: 0.0,
            path_pending: false,
            pending_dest: [0.0, 0.0, 0.0],
            vault_t: None,
            vault_from: None,
            vault_to: None,
            lod_irrelevant: false,
            anim_skip: 0,
            anim_accum: 0.0,
            clip: Clip::Idle,
        };
        (agent, animator_rng)
    }

    /// `get eye()`. `agent.js:263-265`.
    pub fn eye(&self) -> [f64; 3] {
        [self.position[0], self.position[1] + self.eye_height, self.position[2]]
    }

    /// A [`super::squad::MemberSnapshot`] built from this agent's current
    /// state — the read view `Squad::update`/`Squad::can_flank` need.
    pub fn snapshot(&self) -> MemberSnapshot {
        MemberSnapshot {
            id: self.id,
            alive: self.alive,
            state: self.state,
            has_target: self.has_target,
            target_visible: self.target_visible,
            last_known: self.last_known,
            last_known_age: self.last_known_age,
            position: self.position,
        }
    }

    /// The [`Neighbor`] view `_move`'s avoidance loop reads off this agent.
    pub fn as_neighbor(&self) -> Neighbor {
        Neighbor { id: self.id, alive: self.alive, position: self.position, radius: self.radius }
    }

    /// Apply a [`ContactBroadcast`] this agent received from its squad.
    /// `squad.js:64-69`'s writes onto `m`.
    pub fn receive_squad_contact(&mut self, c: ContactBroadcast) {
        self.last_known = c.position;
        self.last_known_age = c.last_known_age;
        self.alertness = 1.0;
        if self.state == AgentState::Idle || self.state == AgentState::Patrol {
            self.set_state(AgentState::Alert);
        }
    }

    /* ================================================================== */
    /* frame                                                              */
    /* ================================================================== */

    /// `update(dt, ctx)`. `agent.js:267-287`.
    pub fn update(&mut self, dt: f64, w: &mut AgentCtx) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        if !self.alive {
            return events;
        }
        self.state_time += dt;
        self.suppression = f64::max(0.0, self.suppression - dt * 0.55);
        self.fire_cooldown -= dt;
        self.burst_cooldown -= dt;
        self.grenade_cooldown -= dt;
        self.peek_timer -= dt;
        self.repath_timer -= dt;
        self.vault_cooldown -= dt;
        if self.last_known_age < 1e6 {
            self.last_known_age += dt;
        }

        // a path the frame budget deferred: ask again before anything else does
        if self.path_pending {
            let dest = self.pending_dest;
            self.go_to(w, dest);
        }

        self.sense(dt, w.player, w.phys);
        self.think(dt, w, &mut events);
        self.move_step(dt, w);
        self.shoot(dt, w, &mut events);
        self.drive(dt, w);
        events
    }

    /* ================================================================== */
    /* perception                                                         */
    /* ================================================================== */

    /// `_sense(dt)`. `agent.js:293-327`. `player` is `ai.playerPosition(...)`
    /// — `None` when there is no player to sense, matching the source's
    /// `if (!player) return;`.
    pub fn sense(&mut self, dt: f64, player: Option<[f64; 3]>, phys: Option<&dyn WorldProbe>) {
        let Some(player) = player else { return };
        let eye = self.eye();
        let to = [player[0] - eye[0], player[1] - eye[1], player[2] - eye[2]];
        let dist = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
        let mut visible = false;
        if dist < self.view_range {
            // `to.multiplyScalar(1 / dist)` — a reciprocal multiply, NOT a
            // divide: `a * (1/d)` and `a / d` differ in the last bit.
            let inv = 1.0 / dist;
            let to_n = [to[0] * inv, to[1] * inv, to[2] * inv];
            let fwd = [self.yaw.sin(), 0.0, self.yaw.cos()];
            let dot = fwd[0] * to_n[0] + fwd[2] * to_n[2];
            // peripheral vision widens once alerted
            let cone = if self.has_target { -0.2 } else { self.view_cos - self.alertness * 0.25 };
            if dot > cone || dist < 4.5 {
                visible = match phys {
                    Some(p) => nav::line_of_sight(p, eye, player, mask::SIGHT),
                    None => true,
                };
            }
        }
        self.target_visible = visible;

        if visible {
            // reaction: fast head-on and close, slow at the edge of vision
            let rate = 1.0 / f64::max(0.12, 0.16 + dist * 0.0075 + (1.0 - self.alertness) * 0.28);
            self.awareness = (self.awareness + dt * rate).min(1.0);
            self.last_known = player;
            self.last_known_age = 0.0;
            self.alertness = 1.0;
            if self.awareness >= 1.0 {
                self.has_target = true;
                self.target = Some(player);
            }
        } else {
            self.awareness = (self.awareness - dt * 0.35).max(0.0);
            if self.has_target && self.last_known_age > 6.5 {
                self.has_target = false;
            }
        }
    }

    /// `hear(pos, loudness)`. `agent.js:330-343`. A gunshot or footstep heard
    /// from `pos` with a given loudness (metres).
    pub fn hear(&mut self, pos: [f64; 3], loudness: f64) {
        if !self.alive {
            return;
        }
        let d = distance(self.position, pos);
        if d > loudness {
            return;
        }
        let strength = 1.0 - d / loudness;
        self.alertness = self.alertness.max((0.35 + strength).min(1.0));
        if self.last_known_age > 1.2 || strength > 0.6 {
            self.last_known = pos;
            self.last_known_age = self.last_known_age.min(0.35);
        }
        // hearing alone never grants a target; it turns the head and the body
        self.awareness = (self.awareness + strength * 0.5).min(0.85);
        if self.state == AgentState::Idle || self.state == AgentState::Patrol {
            self.set_state(AgentState::Alert);
        }
    }

    /// `suppress(amount)`. `agent.js:346-350`. Rounds cracking past raise
    /// suppression, which drives the flinch + duck.
    pub fn suppress(&mut self, amount: f64) {
        if !self.alive {
            return;
        }
        self.suppression = (self.suppression + amount).min(1.6);
        self.alertness = 1.0;
    }

    /* ================================================================== */
    /* behaviour                                                          */
    /* ================================================================== */

    /// `_setState(s)`. `agent.js:356-361`.
    pub fn set_state(&mut self, s: AgentState) {
        if self.state == s {
            return;
        }
        self.state = s;
        self.state_time = 0.0;
        if s != AgentState::Combat && s != AgentState::Suppressed {
            self.peeking = false;
        }
    }

    /// `_think(dt)`. `agent.js:363-445`.
    ///
    /// The source opens with `const sq = this.squad;` and never uses it
    /// (`agent.js:364`) — dead in the source, so nothing corresponds to it
    /// here beyond this note.
    pub fn think(&mut self, dt: f64, w: &mut AgentCtx, events: &mut Vec<AgentEvent>) {
        match self.state {
            AgentState::Idle => {
                self.desired_speed = 0.0;
                self.crouch = false;
                if self.has_target {
                    self.enter_combat();
                } else if self.patrol_points.is_some() && self.state_time > 2.5 {
                    self.set_state(AgentState::Patrol);
                }
            }
            AgentState::Patrol => {
                self.crouch = false;
                self.desired_speed = 1.35;
                if self.has_target {
                    self.enter_combat();
                } else if !self.path_pending {
                    // a route point whose path is still queued is not a route
                    // point reached: taking the next one here would walk the
                    // patrol index forward for free
                    if !self.has_move_target || distance(self.position, self.move_target) < 1.1 {
                        let next = self.patrol_points.as_ref().and_then(|pts| {
                            (!pts.is_empty()).then(|| pts[self.patrol_index % pts.len()])
                        });
                        match next {
                            Some(p) => {
                                self.patrol_index += 1;
                                self.go_to(w, p);
                            }
                            None => self.set_state(AgentState::Idle),
                        }
                    }
                }
            }
            AgentState::Alert => {
                self.crouch = false;
                self.desired_speed = 1.5;
                if self.has_target {
                    self.enter_combat();
                } else {
                    // move to the last known position, then look around
                    if self.last_known_age < 8.0 && !self.has_move_target {
                        let lk = self.last_known;
                        self.go_to(w, lk);
                    }
                    if self.state_time > 12.0 {
                        self.set_state(if self.patrol_points.is_some() {
                            AgentState::Patrol
                        } else {
                            AgentState::Idle
                        });
                    }
                }
            }
            AgentState::Combat => self.combat(dt, w, events),
            AgentState::Suppressed => {
                self.crouch = true;
                self.desired_speed = 0.0;
                self.want_fire = false;
                self.peeking = false;
                if self.suppression < 0.45 {
                    self.set_state(AgentState::Combat);
                }
            }
            AgentState::Flank => {
                self.crouch = false;
                self.desired_speed = 4.4;
                self.want_fire = false;
                if !self.has_move_target
                    || distance(self.position, self.move_target) < 1.2
                    || self.state_time > 7.0
                {
                    self.set_state(AgentState::Combat);
                    self.cover = None;
                }
                if self.suppression > 1.0 {
                    self.set_state(AgentState::Combat);
                }
            }
            AgentState::Retreat => {
                self.crouch = false;
                self.desired_speed = 4.6;
                self.want_fire = false;
                if !self.has_move_target || distance(self.position, self.move_target) < 1.2 {
                    self.set_state(AgentState::Combat);
                }
                if self.health > 45.0 && self.state_time > 4.0 {
                    self.set_state(AgentState::Combat);
                }
            }
            // `switch` has no `dead` case in the source: a dead agent never
            // reaches `_think` (`update` returns early on `!this.alive`).
            AgentState::Dead => {}
        }

        if self.suppression > 1.15 && self.state == AgentState::Combat && self.cover.is_some() {
            self.set_state(AgentState::Suppressed);
        }
    }

    /// `_enterCombat()`. `agent.js:447-451`.
    fn enter_combat(&mut self) {
        self.set_state(AgentState::Combat);
        self.cover = None;
        self.repath_timer = 0.0;
    }

    /// `_combat(dt)`. `agent.js:453-578`.
    fn combat(&mut self, dt: f64, w: &mut AgentCtx, events: &mut Vec<AgentEvent>) {
        // `this.hasTarget ? this.lastKnown : this.lastKnownAge < 5 ? this.lastKnown : null`
        let target = if self.has_target {
            Some(self.last_known)
        } else if self.last_known_age < 5.0 {
            Some(self.last_known)
        } else {
            None
        };
        let Some(target) = target else {
            self.set_state(AgentState::Alert);
            return;
        };
        let dist = distance(self.position, target);

        // wounded and outgunned: fall back
        if self.health < 34.0 && self.state_time > 1.5 && self.rng.float() < dt * 0.5 {
            // `copy(position).sub(target).setY(0).normalize().multiplyScalar(9).add(position)`
            let d = [self.position[0] - target[0], 0.0, self.position[2] - target[2]];
            let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            // Three's `normalize()` is `divideScalar(length() || 1)`, i.e. a
            // multiply by the reciprocal, and a zero length leaves the vector
            // untouched rather than producing NaN.
            let inv = 1.0 / jsmath::or_one(l);
            let away = [
                d[0] * inv * 9.0 + self.position[0],
                d[1] * inv * 9.0 + self.position[1],
                d[2] * inv * 9.0 + self.position[2],
            ];
            if self.go_to(w, away) {
                self.set_state(AgentState::Retreat);
                return;
            }
        }

        // no cover yet, or the current one no longer protects: find one
        if self.cover.is_none() || self.repath_timer <= 0.0 {
            // `squad: sq?.members` — absent entirely when there is no squad.
            let squad_slice = w.squad_positions;
            let pick = w.cover.as_deref_mut().and_then(|c| {
                c.pick(
                    self.position,
                    target,
                    CoverRequest {
                        id: self.id,
                        squad: squad_slice,
                        min_range: 7.0,
                        max_range: 30.0,
                        max_travel: if self.cover.is_some() { 12.0 } else { 26.0 },
                    },
                )
            });
            self.repath_timer = self.rng.range(2.2, 4.5);
            if let Some(pick) = pick {
                // `pick !== this.cover` is an object-identity test in the
                // source; the point's index is that identity here.
                if self.cover.map(|c| c.index) != Some(pick.index) {
                    self.cover = Some(pick);
                    self.cover_pos = [pick.x, pick.y, pick.z];
                    let cp = self.cover_pos;
                    self.go_to(w, cp);
                }
            }
        }

        // A cover point we cannot actually reach must not mute the agent for
        // ever. `_goTo` fails outright when A* finds no route, and a path can
        // also run out short of the point. The branch below reads "has cover,
        // not standing in it" as "walk, weapon down, hold fire", so without
        // this the agent stands in the open with the player in plain sight and
        // never pulls the trigger. (`agent.js:494-509`.)
        if self.cover.is_some()
            && !self.has_move_target
            && !self.path_pending
            && distance(self.position, self.cover_pos) > 0.85
        {
            self.cover = None;
            if let Some(c) = w.cover.as_deref_mut() {
                c.release(self.id);
            }
            self.repath_timer = self.repath_timer.min(0.6);
        }

        let at_cover =
            self.cover.is_some() && distance(self.position, self.cover_pos) < 0.85;

        if self.cover.is_some() && !at_cover {
            // moving into position: run, weapon down, no shooting
            self.desired_speed = 4.3;
            self.crouch = false;
            self.want_fire = false;
            self.aim_weight = 0.35;
        } else {
            self.desired_speed = 0.0;
            self.has_move_target = false;
            // peek-and-shoot, gated by the squad so they alternate
            let allowed = match w.squad.as_deref_mut() {
                Some(sq) => sq.request_peek(self.id, dt),
                None => true,
            };
            if self.peek_timer <= 0.0 {
                // `allowed && this.targetVisible !== false` — `targetVisible`
                // is only ever a boolean here, so this is `allowed &&
                // targetVisible`.
                self.peeking = allowed && self.target_visible;
                self.peek_timer = if self.peeking {
                    self.rng.range(1.1, 2.4)
                } else {
                    self.rng.range(0.7, 1.8)
                };
                if self.peeking {
                    if let (Some(cover), Some(c), Some(phys)) =
                        (self.cover, w.cover.as_deref_mut(), w.phys)
                    {
                        let (side, pos) = c.peek_offset(&cover, target, self.eye_height, phys);
                        self.peek_side = side;
                        self.cover_pos = pos;
                    }
                }
            }
            self.crouch = match self.cover {
                Some(c) => !c.high || !self.peeking,
                None => false,
            };
            self.aim_weight = if self.peeking { 1.0 } else { 0.55 };
            self.want_fire =
                self.peeking && self.target_visible && self.has_target && dist < self.weapon_range;
            // suppressing fire at the last known spot even without a clean shot
            if !self.want_fire && self.has_target && self.last_known_age < 2.2 && self.peeking {
                self.want_fire = self.rng.float() < 0.35;
            }
        }

        // flank when the player has been static and we have friends shooting.
        //
        // Source quirk carried forward deliberately: `agent.js:547` reads
        // `this.grenadeCooldown < 0 === false`, which JS parses as
        // `(grenadeCooldown < 0) === false` — "the cooldown is not negative" —
        // because relational operators bind tighter than equality. It reads
        // like a leftover, not an intentional ammo check, but the recipe says
        // port the behaviour and pin it, not silently fix it.
        let has_squad = w.squad.is_some();
        if has_squad {
            // `(grenadeCooldown < 0) === false` — spelled `!(...)` here because
            // that is the same predicate without tripping `clippy::bool_comparison`.
            let grenade_quirk_gate = !(self.grenade_cooldown < 0.0);
            let gate = self.state_time > 4.0
                && grenade_quirk_gate
                && w.squad.as_deref_mut().is_some_and(|sq| sq.can_flank(self.id))
                && self.rng.float() < dt * 0.25;
            if gate {
                let side = if self.rng.float() < 0.5 { 1.0 } else { -1.0 };
                // `copy(target).sub(position).setY(0).normalize()`
                let p = [target[0] - self.position[0], 0.0, target[2] - self.position[2]];
                let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                let inv = 1.0 / jsmath::or_one(l);
                let perp = [p[0] * inv, p[1] * inv, p[2] * inv];
                let r = self.rng.range(8.0, 15.0);
                // `.set(-perp.z*side, 0, perp.x*side).multiplyScalar(r)
                //  .add(position).addScaledVector(perp, 4)` — transcribed in
                // the source's grouping and order; folding these terms would
                // change the last bits.
                let flank = [
                    -perp[2] * side * r + self.position[0] + perp[0] * 4.0,
                    0.0 * r + self.position[1] + perp[1] * 4.0,
                    perp[0] * side * r + self.position[2] + perp[2] * 4.0,
                ];
                if self.go_to(w, flank) {
                    self.cover = None;
                    if let Some(c) = w.cover.as_deref_mut() {
                        c.release(self.id);
                    }
                    self.set_state(AgentState::Flank);
                    if let Some(sq) = w.squad.as_deref_mut() {
                        sq.claim_flank(self.id);
                    }
                    return;
                }
            }
        }

        // grenade when the player is pinned and we have line of fire
        if self.has_grenade
            && self.grenade_cooldown <= 0.0
            && dist > 8.0
            && dist < 26.0
            && self.last_known_age < 1.5
            && match w.squad.as_deref_mut() {
                Some(sq) => sq.request_grenade(),
                None => true,
            }
        {
            self.throw_grenade(target, w, events);
        }
    }

    /* ================================================================== */
    /* movement                                                           */
    /* ================================================================== */

    /// `_goTo(dest)`. `agent.js:584-610`. `w.path` is `None` for the source's
    /// `if (!grid)` fast path.
    ///
    /// Takes the whole [`AgentCtx`] rather than just the path source, and
    /// reborrows `w.path` internally. That is a borrow-checker requirement, not
    /// a style choice: `AgentCtx<'a>`'s field is
    /// `Option<&'a mut (dyn PathSource + 'a)>`, so passing `w.path.as_deref_mut()`
    /// as an `Option<&mut dyn PathSource>` would have to shorten the trait
    /// object's lifetime *behind* a `&mut`. `&mut` is invariant over its
    /// parameter, so the compiler instead pins the reborrow to `'a` and every
    /// later `&mut w` in the calling function conflicts with it — ten errors
    /// across `update`, `think`, `combat` and `drive`. Reborrowing inside the
    /// callee keeps the borrow local and ends it at the return.
    pub fn go_to(&mut self, w: &mut AgentCtx<'_>, dest: [f64; 3]) -> bool {
        let Some(src) = w.path.as_deref_mut() else {
            self.move_target = dest;
            self.has_move_target = true;
            return true;
        };
        let Some(found) = src.request_path(self.position, dest) else {
            // The frame's A* budget is spent. Hold the destination and retry
            // on the next frame instead of failing outright: `_combat` reads a
            // failed `_goTo` as "that cover point is unreachable" and drops it.
            self.pending_dest = dest;
            self.path_pending = true;
            return false;
        };
        self.path_pending = false;
        if found.is_empty() {
            self.has_move_target = false;
            return false;
        }
        self.path_len = found.len();
        self.path_index = 0;
        self.move_target = found[found.len() - 1];
        self.path = found;
        self.has_move_target = true;
        true
    }

    /// `_move(dt)`. `agent.js:612-703`.
    pub fn move_step(&mut self, dt: f64, w: &mut AgentCtx) -> MoveStep {
        let wp = (self.has_move_target && self.path_index < self.path_len)
            .then(|| self.path[self.path_index]);
        let mut steer = [0.0f64, 0.0, 0.0];
        let mut want = 0.0f64;

        if let Some(wp) = wp {
            let to = [wp[0] - self.position[0], 0.0, wp[2] - self.position[2]];
            let d = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
            let threshold = if self.path_index == self.path_len - 1 { 0.45 } else { 0.75 };
            if d < threshold {
                self.path_index += 1;
                if self.path_index >= self.path_len {
                    self.has_move_target = false;
                }
            } else {
                let inv = 1.0 / d;
                steer = [to[0] * inv, to[1] * inv, to[2] * inv];
                want = self.desired_speed;
            }
        }

        // local avoidance: push off squadmates and steer around them
        for n in w.neighbors {
            if n.id == self.id || !n.alive {
                continue;
            }
            let dx = self.position[0] - n.position[0];
            let dz = self.position[2] - n.position[2];
            let d2 = dx * dx + dz * dz;
            let rr = (self.radius + n.radius + 0.42) * (self.radius + n.radius + 0.42);
            if d2 > rr || d2 < 1e-6 {
                continue;
            }
            let d = d2.sqrt();
            let push = (1.0 - d / rr.sqrt()) * 1.5;
            steer[0] += (dx / d) * push;
            steer[2] += (dz / d) * push;
            // tangential bias breaks head-on deadlocks deterministically.
            // `this.id % 2 ? 1 : -1` — JS `%` keeps the sign of the dividend,
            // and any non-zero value is truthy, so a negative odd id also
            // takes the `1` arm.
            let bias = if self.id % 2 != 0 { 1.0 } else { -1.0 };
            steer[0] += (-dz / d) * push * 0.35 * bias;
            steer[2] += (dx / d) * push * 0.35 * bias;
            if want == 0.0 {
                want = self.desired_speed * 0.35;
            }
        }

        let steer_len_sq = steer[0] * steer[0] + steer[1] * steer[1] + steer[2] * steer[2];
        if steer_len_sq > 1e-6 {
            let l = steer_len_sq.sqrt();
            let inv = 1.0 / jsmath::or_one(l);
            steer = [steer[0] * inv, steer[1] * inv, steer[2] * inv];
        }

        // speed: ease toward the request so starts and stops have weight
        let target_speed =
            want * (if self.crouch { 0.42 } else { 1.0 }) * (1.0 - self.suppression * 0.25);
        self.speed += (target_speed - self.speed) * (dt * 7.0).min(1.0);
        if self.speed < 0.05 {
            self.speed = 0.0;
        }

        // facing: look where we are going, or at the threat when engaged
        let engaged = self.state == AgentState::Combat
            || self.state == AgentState::Suppressed
            || self.has_target;
        if engaged && self.last_known_age < 8.0 {
            self.target_yaw = (self.last_known[0] - self.position[0])
                .atan2(self.last_known[2] - self.position[2]);
        } else if self.speed > 0.2 {
            self.target_yaw = steer[0].atan2(steer[2]);
        }
        let mut dy = self.target_yaw - self.yaw;
        while dy > std::f64::consts::PI {
            dy -= std::f64::consts::PI * 2.0;
        }
        while dy < -std::f64::consts::PI {
            dy += std::f64::consts::PI * 2.0;
        }
        // a big turn while standing still becomes a real turn-in-place step
        if dy.abs() > 0.9 && self.speed < 0.3 {
            w.animator.turn(if dy > 0.0 { 1.0 } else { -1.0 });
        }
        let turn_rate = if self.speed > 0.3 { 6.5 } else { 3.4 };
        self.yaw += f64::max(-turn_rate * dt, f64::min(turn_rate * dt, dy));

        /* integrate through the character controller */
        if self.has_controller && w.controller.is_some() {
            let g = w.gravity;
            self.velocity[1] += g * dt;
            let vx = steer[0] * self.speed;
            let vz = steer[2] * self.speed;
            let height = if self.crouch { 1.16 * self.scale } else { self.height };
            let (pos, grounded, blocked) = {
                let c = w.controller.as_deref_mut().expect("controller present");
                c.set_height(height);
                c.move_by(vx * dt, self.velocity[1] * dt, vz * dt);
                (c.position(), c.grounded(), c.last_move_blocked())
            };
            self.position = pos;
            self.grounded = grounded;
            if grounded && self.velocity[1] < 0.0 {
                self.velocity[1] = 0.0;
            }

            // blocked by something low: vault it
            if blocked && self.speed > 1.5 && self.vault_cooldown <= 0.0 && self.grounded {
                self.try_vault(w);
            }
            if blocked && self.speed > 0.5 {
                self.stuck_timer += dt;
                if self.stuck_timer > 1.1 {
                    self.stuck_timer = 0.0;
                    self.repath_timer = 0.0;
                    if self.has_move_target {
                        let mt = self.move_target;
                        self.go_to(w, mt);
                    }
                }
            } else {
                self.stuck_timer = 0.0;
            }
        } else {
            self.position[0] += steer[0] * self.speed * dt;
            self.position[2] += steer[2] * self.speed * dt;
        }

        MoveStep { steer, speed: self.speed }
    }

    /// `_tryVault()`. `agent.js:705-728`.
    pub fn try_vault(&mut self, w: &mut AgentCtx) {
        let Some(phys) = w.phys else { return };
        let fwd = [self.yaw.sin(), 0.0, self.yaw.cos()];
        let low = phys.raycast(
            [self.position[0], self.position[1] + 0.35, self.position[2]],
            [fwd[0], 0.0, fwd[2]],
            0.85,
            mask::WORLD,
        );
        if low.is_none() {
            return;
        }
        let high = phys.raycast_any(
            [self.position[0], self.position[1] + 1.25, self.position[2]],
            [fwd[0], 0.0, fwd[2]],
            1.1,
            mask::WORLD,
        );
        if high {
            return; // a wall, not a ledge
        }
        // landing spot on the other side
        let lx = self.position[0] + fwd[0] * 1.5;
        let lz = self.position[2] + fwd[2] * 1.5;
        let y = w.ground.ground_at(lx, lz, self.position[1] + 2.2);
        if !y.is_finite() || (y - self.position[1]).abs() > 1.3 {
            return;
        }
        self.vault_cooldown = 2.5;
        w.animator.vault(0.8);
        self.vault_from = Some(self.position);
        self.vault_to = Some([lx, y, lz]);
        self.vault_t = Some(0.0);
    }

    /* ================================================================== */
    /* shooting                                                           */
    /* ================================================================== */

    /// `_shoot(dt)`. `agent.js:734-773`.
    pub fn shoot(&mut self, dt: f64, w: &mut AgentCtx, events: &mut Vec<AgentEvent>) {
        // where the gun is pointing: lead toward the target with human error
        let t = (self.has_target || self.last_known_age < 3.0).then_some(self.last_known);
        match t {
            Some(t) => {
                // aim at the chest, not the feet
                let mut v = [t[0], t[1] + 0.05, t[2]];
                let dist = distance(self.position, v);
                let wobble_t = w.elapsed * 1.7 + self.id as f64;
                let wob = 0.012 + self.suppression * 0.05;
                v[0] += wobble_t.sin() * wob * dist * 0.12;
                v[1] += (wobble_t * 1.7 + 1.1).sin() * wob * dist * 0.08;
                v[2] += (wobble_t * 0.8).cos() * wob * dist * 0.12;
                lerp_into(&mut self.aim_target, v, (dt * 6.0).min(1.0));
            }
            None => {
                let fwd = [self.yaw.sin(), 0.0, self.yaw.cos()];
                let v2 = [
                    self.position[0] + fwd[0] * 12.0,
                    self.position[1] + self.eye_height - 0.1,
                    self.position[2] + fwd[2] * 12.0,
                ];
                lerp_into(&mut self.aim_target, v2, (dt * 3.0).min(1.0));
            }
        }

        if !self.want_fire || w.animator.reloading() || w.animator.vaulting() {
            return;
        }
        if self.ammo <= 0 {
            w.animator
                .reload(if self.variant_name == "irregular" { 2.9 } else { 2.35 });
            events.push(AgentEvent::Reload);
            self.ammo = self.mag_size;
            return;
        }
        if self.burst_left <= 0 {
            if self.burst_cooldown > 0.0 {
                return;
            }
            self.burst_left = self.rng.int(3, 7);
            self.burst_cooldown = self.rng.range(0.45, 1.35) + self.suppression * 0.5;
        }
        if self.fire_cooldown > 0.0 {
            return;
        }
        self.fire_cooldown = 1.0 / self.fire_rate;
        self.burst_left -= 1;
        self.ammo -= 1;
        self.fire_round(w, events);
    }

    /// `_fireRound()`. `agent.js:775-787`.
    ///
    /// Takes the whole [`AgentCtx`] rather than `&mut dyn AgentAnimator` for
    /// the same reason [`Agent::go_to`] does: reborrowing the trait object out
    /// of the ctx at the call site would have to shorten its lifetime behind a
    /// `&mut`, which is invariant. Reborrowing inside the function is local
    /// and needs no coercion.
    fn fire_round(&mut self, w: &mut AgentCtx<'_>, events: &mut Vec<AgentEvent>) {
        let origin = w.animator.muzzle_world();
        let mut dir = w.animator.muzzle_dir();
        // cone of fire: worse when suppressed, better the longer we have been
        // aiming
        let spread = self.spread * (1.0 + self.suppression * 1.5);
        // Draw order is part of the contract: x, then y, then z. `gauss`
        // caches the second Box-Muller sample, so these three draws consume
        // four `float()`s, not six.
        dir[0] += self.rng.gauss() * spread;
        dir[1] += self.rng.gauss() * spread * 0.8;
        dir[2] += self.rng.gauss() * spread;
        let l = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        let inv = 1.0 / jsmath::or_one(l);
        dir = [dir[0] * inv, dir[1] * inv, dir[2] * inv];
        w.animator.fire(1.0);
        events.push(AgentEvent::Fire { origin, dir });
    }

    /// `_throwGrenade(target)`. `agent.js:789-794`. Takes the ctx for the same
    /// lifetime reason as [`Agent::fire_round`].
    fn throw_grenade(
        &mut self,
        target: [f64; 3],
        w: &mut AgentCtx<'_>,
        events: &mut Vec<AgentEvent>,
    ) {
        self.grenade_cooldown = self.rng.range(16.0, 34.0);
        self.has_grenade = false;
        let from = w.animator.muzzle_world();
        events.push(AgentEvent::Grenade { from, target });
    }

    /* ================================================================== */
    /* damage                                                             */
    /* ================================================================== */

    /// Take a hit. NOTE: named `applyDamage`, not `damage` — the weapon's
    /// damage value is a field on this object and a method of the same name
    /// would be shadowed by it. `agent.js:809-837`.
    ///
    /// * `amount` — post-falloff damage
    /// * `part` — which hitbox was struck
    /// * `point` — world impact point
    /// * `dir` — incident direction (unit), `None` for `undefined`
    #[allow(clippy::too_many_arguments)]
    pub fn apply_damage(
        &mut self,
        amount: f64,
        part: BodyPart,
        point: [f64; 3],
        dir: Option<[f64; 3]>,
        an: &mut dyn AgentAnimator,
        cover: Option<&mut dyn CoverSource>,
        events: &mut Vec<AgentEvent>,
    ) {
        if !self.alive {
            return;
        }
        self.health -= amount;
        self.alertness = 1.0;
        self.suppression = (self.suppression + 0.35).min(1.6);
        // knowing where it came from
        if let Some(dir) = dir {
            let v = [
                point[0] + dir[0] * -14.0,
                point[1] + dir[1] * -14.0,
                point[2] + dir[2] * -14.0,
            ];
            if self.last_known_age > 0.5 {
                self.last_known = v;
                self.last_known_age = 0.4;
            }
        }
        if self.state == AgentState::Idle || self.state == AgentState::Patrol {
            self.set_state(AgentState::Alert);
        }

        if self.health <= 0.0 {
            self.die(Some(point), dir, amount, an, cover, events);
            return;
        }
        // hit reaction by region, with the side the round came from.
        //
        // TRAP: `Math.sign(...) || 1`. JS `Math.sign` is three-valued — it
        // returns `0` for `0` and `-0` for `-0`, both falsy, so a dead-on hit
        // takes the `|| 1` arm. Rust's `f64::signum` returns `1.0` for `0.0`
        // and `-1.0` for `-0.0`, which would flip the reaction on a `-0.0`.
        // `Math.sign(...) || 1` — both halves are JS builtins with Rust
        // look-alikes that are wrong (`f64::signum(-0.0)` is `-1.0`, and a
        // `== 0.0` test misses `NaN`), so both come from `jsmath`.
        let side = match dir {
            Some(d) => jsmath::or_one(jsmath::sign(
                d[0] * self.yaw.cos() - d[2] * self.yaw.sin(),
            )),
            None => 1.0,
        };
        let region = match part {
            BodyPart::Head => HitRegion::Head,
            BodyPart::Arm => {
                if self.side_of(point) < 0.0 { HitRegion::ArmR } else { HitRegion::ArmL }
            }
            BodyPart::Leg => {
                if self.side_of(point) < 0.0 { HitRegion::LegR } else { HitRegion::LegL }
            }
            BodyPart::Torso => HitRegion::Torso,
        };
        an.hit(region, side, (0.5 + amount / 45.0).min(1.4));
        if part == BodyPart::Leg {
            self.speed *= 0.4;
        }
    }

    /// Which side of the body a world point is on: `<0` right, `>0` left.
    /// `_sideOf(p)`. `agent.js:840-844`.
    pub fn side_of(&self, p: [f64; 3]) -> f64 {
        let dx = p[0] - self.position[0];
        let dz = p[2] - self.position[2];
        dx * self.yaw.cos() - dz * self.yaw.sin()
    }

    /// `die(point, dir, amount = 30)`. `agent.js:846-883`.
    ///
    /// The ragdoll hand-off itself (`_makeRagdoll`, `agent.js:891-925`) enters
    /// `physics/ragdoll.js`, which this port does not have — see the module
    /// doc comment. Everything around it is here, including the impulse: a
    /// 5.56 round carries ~4 N·s, so anything in the hundreds launches the
    /// body across the street instead of dropping it.
    #[allow(clippy::too_many_arguments)]
    pub fn die(
        &mut self,
        point: Option<[f64; 3]>,
        dir: Option<[f64; 3]>,
        amount: f64,
        an: &mut dyn AgentAnimator,
        cover: Option<&mut dyn CoverSource>,
        events: &mut Vec<AgentEvent>,
    ) {
        if !self.alive {
            return;
        }
        self.alive = false;
        self.state = AgentState::Dead;
        self.want_fire = false;
        an.set_enabled(false);
        if let Some(c) = cover {
            c.release(self.id);
        }
        // `if (this.controller) this.phys.removeCharacter(this.controller);`
        self.has_controller = false;
        self.colliders.clear();

        let d = dir.unwrap_or([0.0, 0.0, 1.0]);
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let inv = 1.0 / jsmath::or_one(l);
        let k = (1.5 + amount * 0.02).min(5.5);
        let impulse = [d[0] * inv * k, d[1] * inv * k, d[2] * inv * k];
        let hit_point =
            point.unwrap_or([self.position[0], self.position[1] + 1.2, self.position[2]]);

        events.push(AgentEvent::Death { point: hit_point, impulse, headshot: false });
        self.dead_time = Some(0.0);
    }

    /* ================================================================== */
    /* drive the visual                                                   */
    /* ================================================================== */

    /// `_drive(dt)`. `agent.js:931-984`, minus the three `group` writes
    /// (`agent.js:941-943`) which are scene-graph bookkeeping.
    pub fn drive(&mut self, dt: f64, w: &mut AgentCtx) {
        // root motion for a vault
        if let (Some(vt), Some(from), Some(to)) = (self.vault_t, self.vault_from, self.vault_to) {
            if w.animator.vaulting() {
                let vt = vt + dt / 0.8;
                self.vault_t = Some(vt);
                let t = vt.min(1.0);
                self.position = [
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t,
                    from[2] + (to[2] - from[2]) * t,
                ];
                self.position[1] += (t * std::f64::consts::PI).sin() * 0.42;
                if let Some(c) = w.controller.as_deref_mut() {
                    c.teleport_to(self.position[0], self.position[1], self.position[2]);
                }
            }
        }

        let moving = self.speed > 0.25;
        let clip = if self.crouch {
            if moving { Clip::CrouchWalk } else { Clip::CrouchIdle }
        } else if self.speed > 2.6 {
            Clip::Run
        } else if moving {
            Clip::Walk
        } else if self.health < 35.0 {
            Clip::HurtIdle
        } else {
            Clip::Idle
        };
        self.clip = clip;

        w.animator.set_state(AnimatorState {
            clip,
            speed: self.speed,
            crouch: self.crouch,
            aim_target: self.aim_target,
            look_target: if self.has_target || self.last_known_age < 4.0 {
                self.last_known
            } else {
                self.aim_target
            },
            aim_weight: self.aim_weight,
            suppress: (self.suppression * 0.8).min(1.0),
        });

        // ANIMATION RATE LOD. The pose write, the three IK chains and the two
        // foot ground rays are the whole per-actor cost, and for an actor that
        // cannot reach a pixel this frame they buy nothing. Evaluate a third as
        // often and hand the solver the accumulated dt, so the stride phase,
        // the recoil envelope and the reload timeline stay on the same clock.
        self.anim_accum += dt;
        if self.lod_irrelevant {
            if self.anim_skip > 0 {
                self.anim_skip -= 1;
                return;
            }
            self.anim_skip = 2; // one evaluation in three while nothing can see it
        } else {
            self.anim_skip = 0;
        }
        w.animator.update(self.anim_accum, w.elapsed);
        self.anim_accum = 0.0;
    }

    /// Push the hit capsules onto the animated skeleton. `syncHitboxes()`,
    /// `agent.js:987-1000`. Returns the `c.setSegment(...)` calls rather than
    /// making them — there is no collider registry in this slice.
    pub fn sync_hitboxes(&self, an: &dyn AgentAnimator) -> Vec<HitboxSegment> {
        if !self.alive {
            return Vec::new();
        }
        self.colliders
            .iter()
            .map(|c| HitboxSegment { part: c.part, a: an.bone_pos(c.a), b: an.bone_pos(c.b) })
            .collect()
    }

    /// `dispose()`. `agent.js:1002-1008`, minus the scene-graph detach.
    pub fn dispose(&mut self) {
        self.has_controller = false;
        self.colliders.clear();
    }
}

/// `THREE.Vector3.prototype.lerp` — `this.x += (v.x - this.x) * alpha`.
fn lerp_into(a: &mut [f64; 3], b: [f64; 3], alpha: f64) {
    a[0] += (b[0] - a[0]) * alpha;
    a[1] += (b[1] - a[1]) * alpha;
    a[2] += (b[2] - a[2]) * alpha;
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}
