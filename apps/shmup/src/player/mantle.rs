//! Ledge detection and the rooted mantle / vault motion.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/player/mantle.js:1-313` — the whole
//! file.
//!
//! Detection is done entirely with `physics` capsule sweeps and rays — this
//! file never touches triangles itself. Three questions have to be answered
//! before a mantle may commit, and all three are asked every time:
//!
//!  1. is there a near-vertical face in front of me?          (forward sweep)
//!  2. where is its lip, and is the lip a walkable surface?    (downward ray)
//!  3. can my capsule actually stand on the far side of it?    (capsule check)
//!
//! If any answer is no we fall through and the player keeps running into the
//! wall, which is the correct failure mode — a mantle that teleports you into
//! geometry is far worse than one that does not trigger.
//!
//! The motion itself is *rooted*: once committed, the capsule is driven along
//! a parametric curve and collision is not consulted again (the destination
//! was already proven clear).
//!
//! ## The physics seam
//!
//! The source duck-types `physics.raycast`/`physics.capsuleCast` and the
//! character controller's own `c.checkCapsule`. Neither is ported yet (see
//! `crate::player` module doc comment); [`WorldProbe`] and [`LedgeCharacter`]
//! name exactly those calls, in the same spirit as
//! `crate::audio::spatial::WorldProbe`. `movement::CharacterController`
//! (the seam `movement.rs` binds its own character controller through) is a
//! supertrait of [`LedgeCharacter`], so one implementation satisfies both.

use crate::world::palette::Surface;
use crate::player::springs::{clamp01, smoothstep, smootherstep, DEG};
use crate::player::tuning::MOVE;
use crate::player::Vec3;

/// `phys.MASK.CHARACTER` / `phys.MASK.WORLD` — the only two masks `mantle.js`
/// queries against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeMask {
    Character,
    World,
}

/// What a `raycast` found — `{ hit: true, point, normal, surface }`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    pub point: Vec3,
    pub normal: Vec3,
    pub surface: Surface,
}

/// What a `capsuleCast` found — `{ hit: true, normal, distance, surface }`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapsuleHit {
    pub normal: Vec3,
    pub distance: f64,
    pub surface: Surface,
}

/// The world queries `mantle.js`'s `LedgeProbe` makes against `physics`.
pub trait WorldProbe {
    /// `phys.raycast(ox, oy, oz, dx, dy, dz, maxDist, mask)`.
    fn raycast(&self, origin: Vec3, dir: Vec3, max_dist: f64, mask: ProbeMask) -> Option<RayHit>;

    /// `phys.capsuleCast(p0, p1, radius, dir, maxDist, mask)`.
    fn capsule_cast(
        &self,
        p0: Vec3,
        p1: Vec3,
        radius: f64,
        dir: Vec3,
        max_dist: f64,
        mask: ProbeMask,
    ) -> Option<CapsuleHit>;

    /// `phys.checkCapsule(p0, p1, radius, mask)` — a general two-point capsule
    /// overlap test against the world, as opposed to [`LedgeCharacter::
    /// check_capsule`] (which tests *this character's own* capsule at a given
    /// centre/height). Used only by the lean probe (`movement.js:841`).
    fn check_capsule_segment(&self, p0: Vec3, p1: Vec3, radius: f64, mask: ProbeMask) -> bool;
}

/// The character-controller facts `LedgeProbe.probe` reads from `c` — a
/// supertrait of `movement::CharacterController`, which adds everything
/// `movement.js` needs beyond these three.
pub trait LedgeCharacter {
    /// `c.position`.
    fn position(&self) -> Vec3;
    /// `c.radius`.
    fn radius(&self) -> f64;
    /// `c.checkCapsule(x, y, z, height)` — would *this* character's capsule
    /// fit, centred at `(x, y, z)`, occupying `height`?
    fn check_capsule(&self, x: f64, y: f64, z: f64, height: f64) -> bool;
}

/// `LEDGE_NONE` / `LEDGE_VAULT` / `LEDGE_MANTLE`. `mantle.js:26-28`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgeKind {
    None,
    Vault,
    Mantle,
}

/// `ledgeKindName`. `mantle.js:310-312`.
pub fn ledge_kind_name(kind: LedgeKind) -> &'static str {
    match kind {
        LedgeKind::Vault => "vault",
        LedgeKind::Mantle => "mantle",
        LedgeKind::None => "none",
    }
}

/// The `LedgeProbe.result` scratch record, reused across calls — copy anything
/// you keep. `mantle.js:41-53`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LedgeResult {
    pub kind: LedgeKind,
    pub fast: bool,
    pub obstacle_height: f64,
    pub top_y: f64,
    pub lip_x: f64,
    pub lip_z: f64,
    pub land_x: f64,
    pub land_y: f64,
    pub land_z: f64,
    pub distance: f64,
    pub surface: Surface,
}

impl Default for LedgeResult {
    fn default() -> Self {
        LedgeResult {
            kind: LedgeKind::None,
            fast: false,
            obstacle_height: 0.0,
            top_y: 0.0,
            lip_x: 0.0,
            lip_z: 0.0,
            land_x: 0.0,
            land_y: 0.0,
            land_z: 0.0,
            distance: 0.0,
            surface: Surface::Concrete,
        }
    }
}

/// `mantle.js:30-167`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LedgeProbe {
    pub result: LedgeResult,
}

impl LedgeProbe {
    pub fn new() -> Self {
        LedgeProbe::default()
    }

    /// `probe(c, fx, fz, standHeight)`. `mantle.js:63-166`.
    ///
    /// `world` stands in for the source's constructor-injected `this.physics`;
    /// the source's "no physics registered -> LEDGE_NONE" early-out
    /// (`mantle.js:68`) has no Rust equivalent because this port's seam makes
    /// `world` a plain reference rather than an optional field — a caller with
    /// no physics bound simply does not call `probe` at all.
    pub fn probe<C: LedgeCharacter + ?Sized, W: WorldProbe + ?Sized>(
        &mut self,
        world: &W,
        c: &C,
        fx: f64,
        fz: f64,
        stand_height: f64,
    ) -> LedgeKind {
        let m = &MOVE.mantle;
        self.result.kind = LedgeKind::None;

        let pos = c.position();
        let feet_y = pos[1];
        let radius = c.radius();

        // ---- 1. a face in front of us -------------------------------------
        let lo = feet_y + (0.14_f64).max(m.min_height * 0.5) + radius * 0.6;
        let hi = feet_y + (m.max_height + 0.1).min(stand_height - 0.12);
        if hi <= lo {
            return LedgeKind::None;
        }
        let p0 = [pos[0], lo, pos[2]];
        let p1 = [pos[0], lo.max(hi), pos[2]];
        let dir = [fx, 0.0, fz];

        let Some(wall) = world.capsule_cast(p0, p1, radius * 0.86, dir, m.reach, ProbeMask::Character)
        else {
            return LedgeKind::None;
        };
        // Must be a wall, not a ramp or a ceiling, and must face us.
        if wall.normal[1].abs() > 0.55 {
            return LedgeKind::None;
        }
        if wall.normal[0] * fx + wall.normal[2] * fz > -0.3 {
            return LedgeKind::None;
        }

        let wall_dist = wall.distance;
        self.result.surface = wall.surface;

        // ---- 2. find the lip ----------------------------------------------
        let lip_x = pos[0] + fx * (wall_dist + radius * 0.86 + 0.06);
        let lip_z = pos[2] + fz * (wall_dist + radius * 0.86 + 0.06);
        let above = feet_y + m.max_height + 0.35;
        let Some(top) = world.raycast(
            [lip_x, above, lip_z],
            [0.0, -1.0, 0.0],
            m.max_height + 1.4,
            ProbeMask::World,
        ) else {
            return LedgeKind::None;
        };
        if top.normal[1] < 0.62 {
            return LedgeKind::None;
        }

        let top_y = top.point[1];
        let obstacle_height = top_y - feet_y;
        if obstacle_height < m.min_height || obstacle_height > m.max_height {
            return LedgeKind::None;
        }
        self.result.surface = top.surface;

        // ---- 3. is the far side standable? --------------------------------
        let deep_x = lip_x + fx * m.land_depth;
        let deep_z = lip_z + fz * m.land_depth;
        let deep = world.raycast(
            [deep_x, top_y + 0.35, deep_z],
            [0.0, -1.0, 0.0],
            2.6,
            ProbeMask::World,
        );
        let deep_supported = deep.is_some_and(|d| d.point[1] > top_y - 0.14 && d.normal[1] > 0.6);

        let stand = stand_height;
        let mut kind = LedgeKind::None;

        if deep_supported {
            // Wide ledge: stand on top of it.
            if c.check_capsule(deep_x, top_y + 0.02, deep_z, stand) {
                kind = LedgeKind::Mantle;
                self.result.land_x = deep_x;
                self.result.land_y = top_y;
                self.result.land_z = deep_z;
            } else if c.check_capsule(lip_x + fx * 0.3, top_y + 0.02, lip_z + fz * 0.3, stand.min(1.2)) {
                // Only crouch-height clearance up there — still worth mantling.
                kind = LedgeKind::Mantle;
                self.result.land_x = lip_x + fx * 0.3;
                self.result.land_y = top_y;
                self.result.land_z = lip_z + fz * 0.3;
            }
        } else {
            // Thin obstacle: hop the far side. Find the floor beyond it.
            let over_x = lip_x + fx * (m.land_depth + 0.28);
            let over_z = lip_z + fz * (m.land_depth + 0.28);
            let floor = world.raycast([over_x, top_y + 0.3, over_z], [0.0, -1.0, 0.0], 3.2, ProbeMask::World);
            if let Some(floor) = floor {
                if floor.normal[1] > 0.55 && top_y - floor.point[1] < 2.4 {
                    let ly = floor.point[1];
                    if c.check_capsule(over_x, ly + 0.02, over_z, stand) {
                        kind = LedgeKind::Vault;
                        self.result.land_x = over_x;
                        self.result.land_y = ly;
                        self.result.land_z = over_z;
                    }
                }
            }
            if kind == LedgeKind::None
                && c.check_capsule(lip_x + fx * 0.24, top_y + 0.02, lip_z + fz * 0.24, stand)
            {
                kind = LedgeKind::Mantle;
                self.result.land_x = lip_x + fx * 0.24;
                self.result.land_y = top_y;
                self.result.land_z = lip_z + fz * 0.24;
            }
        }
        if kind == LedgeKind::None {
            return LedgeKind::None;
        }

        // Head clearance directly over the lip — do not climb into a soffit.
        if !c.check_capsule(lip_x, top_y + 0.02, lip_z, stand.min(1.15)) {
            return LedgeKind::None;
        }

        self.result.kind = kind;
        self.result.fast = obstacle_height <= m.auto_vault_max;
        self.result.obstacle_height = obstacle_height;
        self.result.top_y = top_y;
        self.result.lip_x = lip_x;
        self.result.lip_z = lip_z;
        self.result.distance = wall_dist;
        kind
    }
}

/// The rooted climb. Evaluates a position on the curve plus the camera garnish
/// that sells it: a dip as the hands go up, a roll onto the leading shoulder
/// and a pull toward the wall at the top. `mantle.js:169-308`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MantleMotion {
    pub active: bool,
    pub kind: LedgeKind,
    pub t: f64,
    pub duration: f64,
    pub start_x: f64,
    pub start_y: f64,
    pub start_z: f64,
    pub top_y: f64,
    pub land_x: f64,
    pub land_y: f64,
    pub land_z: f64,
    pub fx: f64,
    pub fz: f64,
    pub side: f64,
    pub height: f64,
    pub exit_speed: f64,
    pub surface: Surface,

    /// Output — written by `evaluate()`, read by the controller and camera.
    pub px: f64,
    pub py: f64,
    pub pz: f64,
    pub cam_y: f64,
    pub cam_forward: f64,
    pub cam_pitch: f64,
    pub cam_roll: f64,
}

impl Default for MantleMotion {
    fn default() -> Self {
        MantleMotion {
            active: false,
            kind: LedgeKind::None,
            t: 0.0,
            duration: 0.6,
            start_x: 0.0,
            start_y: 0.0,
            start_z: 0.0,
            top_y: 0.0,
            land_x: 0.0,
            land_y: 0.0,
            land_z: 0.0,
            fx: 0.0,
            fz: 1.0,
            side: 1.0,
            height: 0.0,
            exit_speed: 0.0,
            surface: Surface::Concrete,
            px: 0.0,
            py: 0.0,
            pz: 0.0,
            cam_y: 0.0,
            cam_forward: 0.0,
            cam_pitch: 0.0,
            cam_roll: 0.0,
        }
    }
}

impl MantleMotion {
    pub fn new() -> Self {
        MantleMotion::default()
    }

    /// `begin(ledge, c, fx, fz, side, speed)`. `mantle.js:205-236`.
    #[allow(clippy::too_many_arguments)]
    pub fn begin<C: LedgeCharacter + ?Sized>(
        &mut self,
        ledge: &LedgeResult,
        c: &C,
        fx: f64,
        fz: f64,
        side: f64,
        speed: f64,
    ) {
        let m = &MOVE.mantle;
        self.active = true;
        self.t = 0.0;
        self.kind = ledge.kind;
        let pos = c.position();
        self.start_x = pos[0];
        self.start_y = pos[1];
        self.start_z = pos[2];
        self.top_y = ledge.top_y;
        self.land_x = ledge.land_x;
        self.land_y = ledge.land_y;
        self.land_z = ledge.land_z;
        self.height = ledge.obstacle_height;
        self.surface = ledge.surface;
        self.fx = fx;
        self.fz = fz;
        // `side || 1` — zero (never passed by a real caller, but the source
        // guards it) falls back to 1.
        self.side = if side == 0.0 { 1.0 } else { side };

        if ledge.kind == LedgeKind::Vault {
            self.duration = if ledge.fast { m.vault_time } else { m.vault_time * 1.45 };
            self.exit_speed = 2.6_f64.max(speed * 0.88);
        } else if ledge.fast {
            self.duration = m.vault_time * 1.12;
            self.exit_speed = 1.8_f64.max(speed * 0.72);
        } else {
            // Tall mantles are slower — the weight of hauling yourself up.
            let f = clamp01((ledge.obstacle_height - m.auto_vault_max) / (m.max_height - m.auto_vault_max));
            self.duration = m.mantle_time + (m.high_mantle_time - m.mantle_time) * f;
            self.exit_speed = 1.35;
        }
        self.evaluate(0.0);
    }

    /// `mantle.js:238-245`.
    pub fn end(&mut self) {
        self.active = false;
        self.kind = LedgeKind::None;
        self.cam_y = 0.0;
        self.cam_forward = 0.0;
        self.cam_pitch = 0.0;
        self.cam_roll = 0.0;
    }

    /// `get progress()`. `mantle.js:247-249`.
    pub fn progress(&self) -> f64 {
        if self.duration > 0.0 {
            clamp01(self.t / self.duration)
        } else {
            1.0
        }
    }

    /// Advance and write the curve outputs. Returns `true` while still
    /// climbing. `mantle.js:251-257`.
    pub fn step(&mut self, dt: f64) -> bool {
        if !self.active {
            return false;
        }
        self.t += dt;
        let u = self.progress();
        self.evaluate(u);
        self.t < self.duration
    }

    /// `mantle.js:259-262`.
    fn evaluate(&mut self, u: f64) {
        if self.kind == LedgeKind::Vault {
            self.eval_vault(u);
        } else {
            self.eval_mantle(u);
        }
    }

    /// Up first, then in — the shape of pulling yourself onto a roof.
    /// `mantle.js:265-287`.
    fn eval_mantle(&mut self, u: f64) {
        let rise = smootherstep(clamp01(u / 0.62));
        let settle = smootherstep(clamp01((u - 0.58) / 0.42));
        let peak = self.top_y + 0.06;
        let mut y = self.start_y + (peak - self.start_y) * rise;
        y += (self.land_y - peak) * settle;

        // 18% of the horizontal travel happens during the rise (hands reaching
        // in), the rest once the hips clear the lip.
        let lead = smoothstep(clamp01(u / 0.62)) * 0.18;
        let pull = smootherstep(clamp01((u - 0.42) / 0.58)) * 0.82;
        let h = clamp01(lead + pull);
        self.px = self.start_x + (self.land_x - self.start_x) * h;
        self.pz = self.start_z + (self.land_z - self.start_z) * h;
        self.py = y;

        // Camera: dip as the arms load, ride up with a shoulder roll, tiny
        // settle.
        let load = (clamp01(u / 0.34) * std::f64::consts::PI).sin()
            * (1.0 - smoothstep(clamp01((u - 0.5) / 0.5)));
        self.cam_y = -0.075 * load + 0.03 * (clamp01((u - 0.55) / 0.45) * std::f64::consts::PI).sin();
        self.cam_forward = 0.05 * (clamp01(u) * std::f64::consts::PI).sin();
        self.cam_pitch =
            -5.2 * DEG * load + 2.1 * DEG * smoothstep(clamp01((u - 0.6) / 0.4));
        self.cam_roll = self.side * 3.2 * DEG * (clamp01(u) * std::f64::consts::PI).sin();
    }

    /// A single low arc that carries momentum through — a hurdle, not a
    /// climb. `mantle.js:289-307`.
    fn eval_vault(&mut self, u: f64) {
        let h = smoothstep(u);
        self.px = self.start_x + (self.land_x - self.start_x) * h;
        self.pz = self.start_z + (self.land_z - self.start_z) * h;

        let clearance = self.top_y + 0.14;
        let up_t = clamp01(u / 0.42);
        let down_t = clamp01((u - 0.42) / 0.58);
        let mut y = self.start_y + (clearance - self.start_y) * smootherstep(up_t);
        // `(u > 0.42 ? 1 : 0)` — a hard gate, not a smooth blend; ported as the
        // source's literal boolean-to-number cast via `f64::from`.
        y += (self.land_y - clearance) * smootherstep(down_t) * f64::from(u > 0.42);
        self.py = y;

        let arc = (clamp01(u) * std::f64::consts::PI).sin();
        self.cam_y = -0.045 * arc;
        self.cam_forward = 0.04 * arc;
        self.cam_pitch = -3.1 * DEG * arc;
        self.cam_roll = self.side * 2.3 * DEG * arc;
    }
}
