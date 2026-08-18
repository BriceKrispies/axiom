//! Ported from Claude-of-Duty `src/ai/agent.js` — the perception, reaction
//! delay, sound handling and behaviour FSM only. `agent.js` is 38k and most
//! of it is the character's *body*: a `THREE.SkinnedMesh` bound to `rig.js`'s
//! skeleton, `animator.js`-driven aim/look/reload/hit-reaction layers,
//! per-bone hitbox colliders, muzzle-flash/tracer/shell events fired through
//! `weapon.js`'s facade, and a ragdoll hand-off on death. None of that is
//! ported here — `rig.js`, `animator.js`, `parts.js`, `soldier.js`,
//! `clips.js`, `geo.js`, `textures.js`, `weapon.js` and `ai/index.js` are
//! explicitly deferred to a later slice (see `apps/shmup/src/ai/mod.rs`).
//!
//! What **is** ported, faithfully, line for line:
//!
//! - **Perception** — `_sense` (`agent.js:293-327`): the 100 degree cone
//!   test, line-of-sight through the collision world, and the angle/distance
//!   -scaled reaction delay (`awareness` build-up rate). `hear`
//!   (`agent.js:330-343`) and `suppress` (`agent.js:346-350`).
//! - **The behaviour FSM** — `_think`/`_combat`
//!   (`agent.js:363-578`): `idle -> patrol -> alert -> combat -> suppressed
//!   -> flank -> retreat -> dead`, including cover selection
//!   ([`super::nav::CoverMap`]), squad-gated peeking, the flank trigger and
//!   the grenade-throw *decision* (the physics spawn itself — `ai.throwGrenade`,
//!   `agent.js:789-794` — is not ported; see [`Agent::think`]'s doc comment).
//! - **Movement decision** — `_goTo`/`_move`'s steering, per-agent local
//!   avoidance and speed/yaw easing (`agent.js:584-703`), *minus* the swept
//!   character-controller integration and vaulting (`agent.js:674-728`),
//!   which are physics-body concerns behind a controller this slice does not
//!   bind — see [`Agent::move_step`]'s doc comment.
//!
//! What is deliberately **not** carried over, and why:
//!
//! - `_shoot`/`_fireRound` (`agent.js:734-787`): firing needs the animator's
//!   muzzle transform and the weapon facade, both deferred. The *decision*
//!   fields `_combat` sets for `_shoot` to consume (`wantFire`, `aimWeight`,
//!   `crouch`, `peeking`) are still computed here — they are genuinely part
//!   of the behaviour FSM — but `ammo`/`burstLeft`/`fireCooldown`/
//!   `burstCooldown`/`magSize`/`spread`/`weaponDamage`/`fireRate` are dropped
//!   entirely: `_think`/`_combat` never read them, only `_shoot` does.
//! - `applyDamage`/`die`/`_makeRagdoll`/`syncHitboxes`/`_drive` — all body or
//!   ragdoll concerns.
//! - `searchPoint`, `reactionTimer`, `aimActual` (`agent.js:194,196,210`):
//!   declared in the constructor and never read or written anywhere else in
//!   the file — genuinely dead fields, not merely dead *computations*, so
//!   nothing here would be carried into a port; they are omitted.
//! - The module-global `_nextId` counter (`agent.js:90,96`): a hidden mutable
//!   static is both untestable in isolation and against the grain of the
//!   rest of this port (see `crate::rng`'s determinism discipline), so
//!   [`Agent::new`] takes `id` as an explicit argument instead — the same
//!   choice already made for [`super::squad::Squad::new`]'s `id`.
//! - `variantName`/`RIG.eyeHeight * scale`/`radius`/`height`/`scale`/`mass`:
//!   body-sizing concerns. [`Agent::new`] takes `eye_height` and `radius`
//!   directly as constructor arguments (the two of these fields that
//!   perception and local avoidance actually read) rather than deriving them
//!   from an unported `RIG`/variant table.

use crate::rng::Rng;

use super::nav::{self, CoverMap, WorldProbe};
use super::squad::{ContactBroadcast, MemberSnapshot, Squad};
use crate::physics::surfaces::mask;

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

/// Another agent's position/radius/liveness, the only fields `_move`'s local
/// avoidance loop reads off a squadmate. `agent.js:632-649`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Neighbor {
    pub id: i32,
    pub alive: bool,
    pub position: [f64; 3],
    pub radius: f64,
}

/// [`Agent::move_step`]'s result: the steering direction (already normalised
/// when non-zero, matching `this._steer` post-normalisation) and the eased
/// speed for this tick. Position integration through a swept character
/// controller is not ported — see [`Agent::move_step`]'s doc comment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveStep {
    pub steer: [f64; 3],
    pub speed: f64,
}

/// One enemy's perception, reaction delay, sound handling and behaviour FSM.
/// `class Agent`, perception/FSM/movement-decision slice only — see the
/// module doc comment for what is and is not carried over from `agent.js`.
pub struct Agent {
    pub id: i32,
    pub rng: Rng,

    pub position: [f64; 3],
    pub yaw: f64,
    pub target_yaw: f64,
    /// `RIG.eyeHeight * scale` in the source; taken directly here — see the
    /// module doc comment.
    pub eye_height: f64,
    pub radius: f64,

    pub health: f64,
    pub max_health: f64,
    pub alive: bool,
    pub state: AgentState,
    pub state_time: f64,
    pub team: i32,

    /* ---------------- perception ---------------- */
    pub view_range: f64,
    pub view_cos: f64,
    /// 0..1 build-up before the target is acknowledged.
    pub awareness: f64,
    pub has_target: bool,
    pub target_visible: bool,
    pub last_known: [f64; 3],
    pub last_known_age: f64,
    pub suppression: f64,
    pub alertness: f64,

    /* ---------------- combat decision ---------------- */
    pub weapon_range: f64,
    pub aim_target: [f64; 3],
    pub aim_weight: f64,
    pub want_fire: bool,
    pub peek_side: i32,
    pub peeking: bool,
    pub peek_timer: f64,
    pub grenade_cooldown: f64,
    pub has_grenade: bool,

    /* ---------------- navigation ---------------- */
    pub path: Vec<[f64; 3]>,
    pub path_index: usize,
    pub repath_timer: f64,
    pub move_target: [f64; 3],
    pub has_move_target: bool,
    pub desired_speed: f64,
    pub speed: f64,
    pub crouch: bool,
    /// Index into the [`CoverMap`]'s points, mirroring the source's direct
    /// object reference.
    pub cover: Option<usize>,
    pub cover_pos: [f64; 3],
    pub patrol_points: Option<Vec<[f64; 3]>>,
    pub patrol_index: usize,
    pub stuck_timer: f64,
    /// A path request the frame budget pushed to the next frame in the
    /// source (`agent.js:235-237,591-599`). `ai/index.js`'s per-frame A*
    /// budget is not ported (see the module doc comment on `mod.rs`), so
    /// [`Agent::go_to`] never sets this — it is kept only so a future budget
    /// layer has somewhere to write it.
    pub path_pending: bool,
}

impl Agent {
    /// `constructor(ai, opts)`, perception/FSM/movement fields only.
    /// `agent.js:93-257`.
    pub fn new(id: i32, rng: Rng, position: [f64; 3], yaw: f64, eye_height: f64, radius: f64) -> Self {
        Agent {
            id,
            rng,
            position,
            yaw,
            target_yaw: yaw,
            eye_height,
            radius,
            health: 100.0,
            max_health: 100.0,
            alive: true,
            state: AgentState::Idle,
            state_time: 0.0,
            team: 1,
            view_range: 58.0,
            view_cos: ((100.0f64 * std::f64::consts::PI) / 180.0 / 2.0).cos(),
            awareness: 0.0,
            has_target: false,
            target_visible: false,
            last_known: [0.0, 0.0, 0.0],
            last_known_age: f64::INFINITY,
            suppression: 0.0,
            alertness: 0.0,
            weapon_range: 60.0,
            aim_target: [0.0, 0.0, 0.0],
            aim_weight: 0.0,
            want_fire: false,
            peek_side: 0,
            peeking: false,
            peek_timer: 0.0,
            grenade_cooldown: 0.0,
            has_grenade: true,
            path: Vec::new(),
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
            path_pending: false,
        }
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

    /* ================================================================== */
    /* perception                                                         */
    /* ================================================================== */

    /// `_sense(dt)`. `agent.js:293-327`. `player` is `ai.playerPosition(...)`
    /// — `None` when there is no player to sense, matching the source's
    /// `if (!player) return;`.
    pub fn sense(&mut self, dt: f64, player: Option<[f64; 3]>, phys: &dyn WorldProbe) {
        let Some(player) = player else { return };
        let eye = [self.position[0], self.position[1] + self.eye_height, self.position[2]];
        let to = [player[0] - eye[0], player[1] - eye[1], player[2] - eye[2]];
        let dist = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
        let mut visible = false;
        if dist < self.view_range {
            let to_n = [to[0] / dist, to[1] / dist, to[2] / dist];
            let fwd = [self.yaw.sin(), 0.0, self.yaw.cos()];
            let dot = fwd[0] * to_n[0] + fwd[2] * to_n[2];
            // peripheral vision widens once alerted
            let cone = if self.has_target { -0.2 } else { self.view_cos - self.alertness * 0.25 };
            if dot > cone || dist < 4.5 {
                visible = nav::line_of_sight(phys, eye, player, mask::SIGHT);
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
    /// suppression, which drives the flinch/duck (ported elsewhere, in the
    /// deferred animator).
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

    /// `_think(dt)` + `_combat(dt)`. `agent.js:363-578`.
    ///
    /// `squad`/`squad_members` are `None`/`&[]` for a lone agent, matching
    /// the source's `sq = this.squad` (nullable) and its `squad?.` guards.
    /// `squad_members` should include every member of this agent's squad
    /// (their [`Agent::snapshot`]), current as of the start of this frame.
    ///
    /// The grenade branch (`agent.js:568-577`) sets `grenade_cooldown`/
    /// `has_grenade` exactly as `_throwGrenade` does (`agent.js:789-794`) but
    /// does not spawn anything — the physics rigid body and the animator's
    /// fire pose are `ai.throwGrenade`'s job, deferred. Callers can detect a
    /// throw by `has_grenade` going from `true` to `false`.
    #[allow(clippy::too_many_arguments)]
    pub fn think(
        &mut self,
        dt: f64,
        grid: &mut nav::NavGrid,
        cover: &mut CoverMap,
        phys: &dyn WorldProbe,
        squad: Option<&mut Squad>,
        squad_members: &[MemberSnapshot],
    ) {
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
                    // a route point whose path is still queued is not a route point
                    // reached: taking the next one here would walk the patrol index
                    // forward for free
                    if !self.has_move_target || distance(self.position, self.move_target) < 1.1 {
                        let next = self.patrol_points.as_ref().and_then(|pts| {
                            (!pts.is_empty()).then(|| pts[self.patrol_index % pts.len()])
                        });
                        match next {
                            Some(p) => {
                                self.patrol_index += 1;
                                self.go_to(grid, p);
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
                        self.go_to(grid, self.last_known);
                    }
                    if self.state_time > 12.0 {
                        self.set_state(if self.patrol_points.is_some() { AgentState::Patrol } else { AgentState::Idle });
                    }
                }
            }
            AgentState::Combat => self.combat(dt, grid, cover, phys, squad, squad_members),
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
                if !self.has_move_target || distance(self.position, self.move_target) < 1.2 || self.state_time > 7.0 {
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
    #[allow(clippy::too_many_arguments)]
    fn combat(
        &mut self,
        dt: f64,
        grid: &mut nav::NavGrid,
        cover_map: &mut CoverMap,
        phys: &dyn WorldProbe,
        mut squad: Option<&mut Squad>,
        squad_members: &[MemberSnapshot],
    ) {
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
            let mut away = [self.position[0] - target[0], 0.0, self.position[2] - target[2]];
            let l = (away[0] * away[0] + away[2] * away[2]).sqrt();
            if l > 1e-9 {
                away = [away[0] / l * 9.0 + self.position[0], self.position[1], away[2] / l * 9.0 + self.position[2]];
            } else {
                away = self.position;
            }
            if self.go_to(grid, away) {
                self.set_state(AgentState::Retreat);
                return;
            }
        }

        // no cover yet, or the current one no longer protects: find one
        if self.cover.is_none() || self.repath_timer <= 0.0 {
            let squad_pos: Vec<nav::SquadMemberPos> = squad_members
                .iter()
                .map(|m| nav::SquadMemberPos { id: m.id, alive: m.alive, x: m.position[0], z: m.position[2] })
                .collect();
            let squad_pos_opt = (!squad_pos.is_empty()).then_some(squad_pos.as_slice());
            let pick = cover_map.pick(
                self.position,
                target,
                nav::PickOpts {
                    min_range: 7.0,
                    max_range: 30.0,
                    id: self.id,
                    squad: squad_pos_opt,
                    max_travel: if self.cover.is_some() { 12.0 } else { 26.0 },
                    ..nav::PickOpts::default()
                },
            );
            self.repath_timer = self.rng.range(2.2, 4.5);
            if let Some(idx) = pick {
                if self.cover != Some(idx) {
                    self.cover = Some(idx);
                    let p = cover_map.points[idx];
                    self.cover_pos = [p.x, p.y, p.z];
                    self.go_to(grid, self.cover_pos);
                }
            }
        }

        // A cover point we cannot actually reach must not mute the agent for
        // ever (`agent.js:494-509`).
        if self.cover.is_some()
            && !self.has_move_target
            && !self.path_pending
            && distance(self.position, self.cover_pos) > 0.85
        {
            self.cover = None;
            cover_map.release(self.id);
            self.repath_timer = self.repath_timer.min(0.6);
        }

        let at_cover = self.cover.is_some() && distance(self.position, self.cover_pos) < 0.85;

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
            let allowed = squad.as_deref_mut().is_none_or(|sq| sq.request_peek(self.id));
            if self.peek_timer <= 0.0 {
                self.peeking = allowed && self.target_visible;
                self.peek_timer = if self.peeking { self.rng.range(1.1, 2.4) } else { self.rng.range(0.7, 1.8) };
                if self.peeking {
                    if let Some(idx) = self.cover {
                        let p = cover_map.points[idx];
                        let (side, pos) = cover_map.peek_offset(&p, target, self.eye_height, phys);
                        self.peek_side = side;
                        self.cover_pos = pos;
                    }
                }
            }
            self.crouch = self.cover.is_some_and(|idx| !cover_map.points[idx].high || !self.peeking);
            self.aim_weight = if self.peeking { 1.0 } else { 0.55 };
            self.want_fire = self.peeking && self.target_visible && self.has_target && dist < self.weapon_range;
            // suppressing fire at the last known spot even without a clean shot
            if !self.want_fire && self.has_target && self.last_known_age < 2.2 && self.peeking {
                self.want_fire = self.rng.float() < 0.35;
            }
        }

        // flank when the player has been static and we have friends shooting.
        //
        // Source quirk carried forward deliberately: `agent.js:547` reads
        // `this.grenadeCooldown < 0 === false`, which JS parses as
        // `(grenadeCooldown < 0) === false` — i.e. "cooldown is not negative"
        // — because relational operators bind tighter than equality. That is
        // almost always true (the cooldown only dips negative in the single
        // frame after it expires and before a throw resets it), so in
        // practice this gate rarely excludes anything; it reads like a
        // leftover/typo, not an intentional ammo check, but the recipe says
        // port the behaviour and pin it, not silently "fix" it.
        if let Some(sq) = squad.as_deref_mut() {
            let grenade_quirk_gate = (self.grenade_cooldown < 0.0) == false; // NOT `!= false` — see doc comment above
            if self.state_time > 4.0
                && grenade_quirk_gate
                && sq.can_flank(self.id, squad_members)
                && self.rng.float() < dt * 0.25
            {
                let side = if self.rng.float() < 0.5 { 1.0 } else { -1.0 };
                let mut perp = [target[0] - self.position[0], 0.0, target[2] - self.position[2]];
                let l = (perp[0] * perp[0] + perp[2] * perp[2]).sqrt();
                if l > 1e-9 {
                    perp = [perp[0] / l, 0.0, perp[2] / l];
                }
                let r = self.rng.range(8.0, 15.0);
                let flank = [
                    -perp[2] * side * r + self.position[0] + perp[0] * 4.0,
                    self.position[1],
                    perp[0] * side * r + self.position[2] + perp[2] * 4.0,
                ];
                if self.go_to(grid, flank) {
                    self.cover = None;
                    cover_map.release(self.id);
                    self.set_state(AgentState::Flank);
                    sq.claim_flank(self.id);
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
            && squad.as_deref_mut().is_none_or(|sq| sq.request_grenade())
        {
            // `_throwGrenade`'s state changes (`agent.js:789-794`); the actual
            // spawn (`ai.throwGrenade`) is not ported — see this method's doc
            // comment.
            self.grenade_cooldown = self.rng.range(16.0, 34.0);
            self.has_grenade = false;
        }
    }

    /* ================================================================== */
    /* movement                                                           */
    /* ================================================================== */

    /// `_goTo(dest)`. `agent.js:584-610`. The source's frame-budget deferral
    /// (`pathPending`/`ai.requestPath` returning `-1`) is `ai/index.js`'s job
    /// and not ported — see the module doc comment and [`Agent::path_pending`]'s
    /// doc comment; this always resolves a path synchronously against `grid`.
    pub fn go_to(&mut self, grid: &mut nav::NavGrid, dest: [f64; 3]) -> bool {
        let path = grid.find_path(self.position, dest, nav::FindPathOpts::default());
        if path.is_empty() {
            self.has_move_target = false;
            return false;
        }
        self.move_target = path[path.len() - 1];
        self.path = path;
        self.path_index = 0;
        self.has_move_target = true;
        true
    }

    /// `_move(dt)`. `agent.js:612-703`, minus the swept character-controller
    /// integration (`agent.js:674-702`, `c.move(...)`/vaulting) — that binds
    /// to `crate::physics::character::Character` through a controller trait
    /// this slice does not define, since nothing in this slice's scope
    /// (perception/FSM/nav-and-cover) needs the character to actually move,
    /// only to decide *how* it wants to. `position`/`yaw` are still advanced
    /// here (the path-follow/waypoint-advance and turn-rate-limited yaw
    /// easing, which are pure kinematics with no collision response), so a
    /// caller integrating a real controller should read [`MoveStep::steer`]/
    /// `speed` and drive its own `position`, not trust this method's
    /// `self.position`, once a controller is wired.
    pub fn move_step(&mut self, dt: f64, neighbors: &[Neighbor]) -> MoveStep {
        let wp = (self.has_move_target && self.path_index < self.path.len()).then(|| self.path[self.path_index]);
        let mut steer = [0.0, 0.0, 0.0];
        let mut want = 0.0;

        if let Some(wp) = wp {
            let mut to = [wp[0] - self.position[0], 0.0, wp[2] - self.position[2]];
            let d = (to[0] * to[0] + to[2] * to[2]).sqrt();
            let threshold = if self.path_index == self.path.len() - 1 { 0.45 } else { 0.75 };
            if d < threshold {
                self.path_index += 1;
                if self.path_index >= self.path.len() {
                    self.has_move_target = false;
                }
            } else {
                to = [to[0] / d, 0.0, to[2] / d];
                steer = to;
                want = self.desired_speed;
            }
        }

        // local avoidance: push off squadmates and steer around them
        for n in neighbors {
            if n.id == self.id || !n.alive {
                continue;
            }
            let dx = self.position[0] - n.position[0];
            let dz = self.position[2] - n.position[2];
            let d2 = dx * dx + dz * dz;
            let rr = (self.radius + n.radius + 0.42).powi(2);
            if d2 > rr || d2 < 1e-6 {
                continue;
            }
            let d = d2.sqrt();
            let push = (1.0 - d / rr.sqrt()) * 1.5;
            steer[0] += (dx / d) * push;
            steer[2] += (dz / d) * push;
            // tangential bias breaks head-on deadlocks deterministically
            let bias = if self.id % 2 != 0 { 1.0 } else { -1.0 };
            steer[0] += (-dz / d) * push * 0.35 * bias;
            steer[2] += (dx / d) * push * 0.35 * bias;
            if want == 0.0 {
                want = self.desired_speed * 0.35;
            }
        }

        let steer_len_sq = steer[0] * steer[0] + steer[2] * steer[2];
        if steer_len_sq > 1e-6 {
            let l = steer_len_sq.sqrt();
            steer = [steer[0] / l, 0.0, steer[2] / l];
        }

        // speed: ease toward the request so starts and stops have weight
        let target_speed = want * (if self.crouch { 0.42 } else { 1.0 }) * (1.0 - self.suppression * 0.25);
        self.speed += (target_speed - self.speed) * (dt * 7.0).min(1.0);
        if self.speed < 0.05 {
            self.speed = 0.0;
        }

        // facing: look where we are going, or at the threat when engaged
        let engaged = self.state == AgentState::Combat || self.state == AgentState::Suppressed || self.has_target;
        if engaged && self.last_known_age < 8.0 {
            self.target_yaw = (self.last_known[0] - self.position[0]).atan2(self.last_known[2] - self.position[2]);
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
        // a big turn while standing still becomes a real turn-in-place step in
        // the source (`this.animator.turn(...)`) — not ported, see this
        // method's doc comment.
        let turn_rate = if self.speed > 0.3 { 6.5 } else { 3.4 };
        self.yaw += dy.clamp(-turn_rate * dt, turn_rate * dt);

        MoveStep { steer, speed: self.speed }
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}
