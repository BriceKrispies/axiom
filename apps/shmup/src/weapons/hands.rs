//! Ported from Claude-of-Duty `src/weapons/hands.js:1-1163` — the analytic
//! two-bone arm rig (`class Arm`, `hands.js:513-1049`) and the six authored
//! finger/thumb pose tables (`HAND_POSES`, `hands.js:1056-1163`).
//!
//! **Scope of this slice.** `hands.js` is two things bolted together: a
//! *rig* (the two-bone IK solve, the pole vector, the per-weapon
//! contact-fitting search, the finger/thumb curl state) and a *mesh
//! authoring* pipeline (`buildGlove`/`buildFinger`/`buildThumb`/
//! `buildSleeve`, ~450 lines of `geometry::primitives` calls) that turns that
//! rig into the actual glove/sleeve geometry described in the module's
//! anatomy comment. This port carries the **rig**: [`Arm::solve`] (the
//! two-bone IK, `hands.js:999-1042`), its construction (bone-length cheat,
//! rig-space pole vector, shoulder placement, `hands.js:513-658`), and the
//! chirality mirror (`hands.js:583-596`) as a documented, queryable property
//! of an [`Arm`]. It does **not** carry the mesh builders or
//! `Arm::fitToCylinder`/`bakeSurfaceMasks`/`bakeContactAO` — those exist only
//! to place and shade vertex geometry that has no consumer yet (no material
//! binding: `materials.js` is a separate, not-yet-ported file per
//! `docs/work-manifests/shmup-port/05-port-status.md`'s remaining-work
//! list, item 3). `weapons::viewmodel`'s support-hand pose therefore falls
//! back to the *authored* `HAND_POSES.clamp` rather than the JS's
//! per-weapon-solved fit — a documented simplification, not a silent
//! divergence; see that module's doc and
//! `docs/work-manifests/shmup-port/notes/hands.md`.
//!
//! ## The three things this slice has to get right (all in [`Arm`])
//!
//! - **Bone lengths are cheated 10% long.** [`L_UPPER`]/[`L_FORE`] are 330/300
//!   mm, not the anatomical 300/272 — see their doc comment for the reach
//!   arithmetic that forces this.
//! - **The pole vector lives in rig space, not hand space.** [`Arm::pole`] is
//!   a fixed direction in the arm root's parent space (the viewmodel rig's
//!   space) — `hands.js:540`'s comment explains why hand space swings the
//!   support elbow through the near plane. [`Arm::solve`] never transforms
//!   it; the caller is responsible for handing [`Arm::solve`] a target
//!   already expressed in that same space (`viewmodel.rs`'s `solve_hands`
//!   rebases the body-fixed shoulder into rig space every frame for exactly
//!   this reason).
//! - **Chirality is handled by mirroring.** [`Arm::hand_mirror_x`] documents
//!   and reproduces `hands.js:595`'s `handInner.scale.x = side < 0 ? 1 : -1`
//!   decision — the authored glove/finger geometry is a left hand, so it is
//!   the *right* arm that gets mirrored, not the left.

use crate::weapons::rig_math::{Q, V3};

/// `hands.js:43`. See the long derivation there (reproduced in this module's
/// doc): a real 300/272 mm arm locks the two-bone solve at 99.5% extension
/// once the shoulder is far enough back to stay behind the eye, which reads
/// as a broomstick. Cheating both bones 10% long buys 91% extension instead —
/// visible bend, and the extra length pushes the elbow further out of frame
/// rather than into it.
pub const L_UPPER: f64 = 0.33;
/// `hands.js:44`.
pub const L_FORE: f64 = 0.3;

/// One [`HAND_POSES`] entry: per-joint flexion in radians, proximal to distal,
/// one triple per finger (index, middle, ring, little), plus the thumb's two
/// hinges and its base (abduction/rotation) orientation. `hands.js:1056-1163`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandPose {
    pub fingers: [[f64; 3]; 4],
    pub thumb: [f64; 2],
    pub thumb_base: [f64; 3],
}

/// The six authored grip shapes a pose name in [`crate::weapons::clips`] or a
/// weapon's `lhandPose` selects. `hands.js:1056-1163`, one variant per
/// top-level `HAND_POSES` key plus the two the clip vocabulary never names
/// (`grip`, the shooting-hand default; `cup`, the pistol two-handed default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandPoseName {
    /// Firing grip on a pistol grip. `hands.js:1057-1075`.
    Grip,
    /// Support hand wrapped around a handguard, authored (not
    /// contact-solved — see the module doc). `hands.js:1077-1086`.
    Wrap,
    /// C-clamp on a handguard, authored. `hands.js:1101-1129`.
    Clamp,
    /// Two-handed pistol grip, support hand cups the shooting hand.
    /// `hands.js:1131-1140`.
    Cup,
    /// Open hand: mag grab, charging handle, inspect. `hands.js:1142-1151`.
    Open,
    /// Pinch: holding the charging handle or a magazine spine.
    /// `hands.js:1153-1162`.
    Pinch,
}

impl From<crate::weapons::clips::Pose> for HandPoseName {
    /// `clips.js`'s four grip-shape literals are a subset of `HAND_POSES`'
    /// six; `grip`/`cup` are never named by an authored clip key
    /// (`clips.rs`'s `Pose` enum has no variant for them).
    fn from(p: crate::weapons::clips::Pose) -> Self {
        match p {
            crate::weapons::clips::Pose::Wrap => HandPoseName::Wrap,
            crate::weapons::clips::Pose::Pinch => HandPoseName::Pinch,
            crate::weapons::clips::Pose::Open => HandPoseName::Open,
            crate::weapons::clips::Pose::Clamp => HandPoseName::Clamp,
        }
    }
}

/// `HAND_POSES`, `hands.js:1056-1163`, transcribed field-for-field.
pub fn hand_pose(name: HandPoseName) -> HandPose {
    match name {
        HandPoseName::Grip => HandPose {
            fingers: [
                [0.55, 0.72, 0.34],
                [1.15, 1.2, 0.62],
                [1.2, 1.25, 0.65],
                [1.22, 1.28, 0.66],
            ],
            thumb: [0.5, 0.34],
            thumb_base: [0.15, -1.02, -0.62],
        },
        HandPoseName::Wrap => HandPose {
            fingers: [
                [1.18, 1.05, 0.45],
                [1.26, 1.12, 0.5],
                [1.3, 1.16, 0.55],
                [1.34, 1.2, 0.6],
            ],
            thumb: [0.42, 0.3],
            thumb_base: [0.1, -1.15, -0.35],
        },
        HandPoseName::Clamp => HandPose {
            fingers: [
                [0.612, 1.059, 0.797],
                [0.731, 1.286, 0.863],
                [0.73, 1.268, 0.808],
                [0.601, 1.105, 0.684],
            ],
            thumb: [0.3, 0.24],
            thumb_base: [0.04, 0.76, -0.05],
        },
        HandPoseName::Cup => HandPose {
            fingers: [
                [1.05, 0.95, 0.4],
                [1.12, 1.0, 0.44],
                [1.16, 1.04, 0.48],
                [1.2, 1.08, 0.52],
            ],
            thumb: [0.28, 0.2],
            thumb_base: [0.0, -1.25, -0.2],
        },
        HandPoseName::Open => HandPose {
            fingers: [
                [0.35, 0.28, 0.14],
                [0.32, 0.26, 0.12],
                [0.34, 0.28, 0.14],
                [0.4, 0.32, 0.16],
            ],
            thumb: [0.12, 0.1],
            thumb_base: [0.1, -0.8, -0.35],
        },
        HandPoseName::Pinch => HandPose {
            fingers: [
                [0.95, 0.85, 0.55],
                [1.0, 0.9, 0.6],
                [0.7, 0.6, 0.35],
                [0.6, 0.5, 0.3],
            ],
            thumb: [0.62, 0.55],
            thumb_base: [0.25, -0.75, -0.7],
        },
    }
}

/// `new Arm(side, materials, opts)`'s options (`hands.js:514-529`), minus
/// `materials` — this port carries no mesh, so nothing here needs a material
/// binding. Field defaults match the source's `opts.x ?? default` chain.
#[derive(Debug, Clone, Copy)]
pub struct ArmOpts {
    pub scale: f64,
    pub upper: f64,
    pub fore: f64,
    pub shoulder_x: f64,
    pub shoulder_y: f64,
    pub shoulder_z: f64,
    pub pose: HandPoseName,
}

impl Default for ArmOpts {
    /// `hands.js:516-529`'s defaults: `scale=1, upper=L_UPPER, fore=L_FORE,
    /// shoulderX=0.19, shoulderY=-0.19, shoulderZ=0.12, pose='wrap'`.
    fn default() -> Self {
        ArmOpts {
            scale: 1.0,
            upper: L_UPPER,
            fore: L_FORE,
            shoulder_x: 0.19,
            shoulder_y: -0.19,
            shoulder_z: 0.12,
            pose: HandPoseName::Wrap,
        }
    }
}

/// One arm: shoulder -> upper -> fore -> hand, solved from the hand target.
/// `class Arm`, `hands.js:513-1049` (rig subset — see module doc).
#[derive(Debug, Clone)]
pub struct Arm {
    /// `-1` left, `+1` right. `hands.js:515`.
    pub side: f64,
    pub scale: f64,
    pub l1: f64,
    pub l2: f64,
    /// Body-fixed shoulder, in the arm root's parent space. `hands.js:525-529`.
    pub shoulder: V3,
    /// Elbow-swing direction, in the arm root's space — **not** hand space.
    /// See the module doc's "pole vector" bullet. `hands.js:540`.
    pub pole: V3,

    /// The current solved hand target, set by [`Arm::solve`]. `hands.js:695-696`.
    pub hand_pos: V3,
    pub hand_quat: Q,
    /// Upper-arm pivot: position = shoulder, orientation aims the bone at the
    /// elbow. `hands.js:1031-1033`.
    pub upper_pos: V3,
    pub upper_quat: Q,
    pub elbow: V3,
    /// Forearm pivot: position = elbow, orientation aims the bone at the
    /// hand, rolled toward the back of the hand. `hands.js:1037-1040`.
    pub fore_pos: V3,
    pub fore_quat: Q,

    /// Current finger/thumb curl pose. `hands.js:972-983`.
    pub pose: HandPose,
    pub pose_name: HandPoseName,
    /// Trigger-finger curl the last [`Arm::set_trigger`] call computed —
    /// `hands.js:986-993`'s per-joint drive, stored rather than applied to a
    /// mesh joint (no mesh — see module doc).
    pub trigger_curl: [f64; 3],
}

impl Arm {
    /// `constructor(side, materials, opts)`, minus mesh construction.
    /// `hands.js:514-658`.
    pub fn new(side: f64, opts: ArmOpts) -> Self {
        let l1 = opts.upper * opts.scale;
        let l2 = opts.fore * opts.scale;
        let shoulder = V3::new(side * opts.shoulder_x, opts.shoulder_y, opts.shoulder_z);
        let pole = V3::new(side * 0.46, -0.86, 0.22).normalize();
        let pose = hand_pose(opts.pose);
        Arm {
            side,
            scale: opts.scale,
            l1,
            l2,
            shoulder,
            pole,
            hand_pos: V3::ZERO,
            hand_quat: Q::IDENTITY,
            upper_pos: shoulder,
            upper_quat: Q::IDENTITY,
            elbow: shoulder,
            fore_pos: shoulder,
            fore_quat: Q::IDENTITY,
            pose,
            pose_name: opts.pose,
            trigger_curl: [0.55, 0.72, 0.34],
        }
    }

    /// `hands.js:595`: `this.handInner.scale.x = side < 0 ? 1 : -1`. The
    /// authored glove/finger geometry (not carried by this port — see module
    /// doc) is a left hand; mirroring the **right** arm's local X axis is
    /// what turns it into a correctly-chiral right hand, and getting this
    /// backwards puts the trigger finger at the bottom-rear of the grip
    /// instead of on the trigger face (`hands.js:585-594`'s comment). Kept
    /// here as a queryable, tested property of the rig even without a mesh
    /// consumer, so a future mesh port reads the decision off the type that
    /// owns it rather than re-deriving it.
    pub fn hand_mirror_x(&self) -> f64 {
        if self.side < 0.0 {
            1.0
        } else {
            -1.0
        }
    }

    /// `setPose(name)`. `hands.js:972-983` (finger/thumb curl only — no mesh
    /// joints to write into).
    pub fn set_pose(&mut self, name: HandPoseName) {
        self.pose = hand_pose(name);
        self.pose_name = name;
    }

    /// `setTrigger(t)`. `hands.js:986-993`. `t` is 0 (off the trigger) to 1
    /// (fully pressed); the rest pose (`t=0`) matches `HAND_POSES.grip`'s
    /// index-finger curl, already on the trigger with slack taken up.
    pub fn set_trigger(&mut self, t: f64) {
        self.trigger_curl = [-(0.55 + t * 0.3), -(0.72 + t * 0.42), -(0.34 + t * 0.3)];
    }

    /// Orient a bone whose geometry runs along its local -Z so that -Z points
    /// along `dir`, with local +Y rolled toward `up`. `aimBone`,
    /// `hands.js:493-506`. Deliberately does not go through `Quaternion.
    /// setFromRotationMatrix` on a materialised matrix — see
    /// `Q::from_basis`'s doc.
    fn aim_bone(dir: V3, up: V3) -> Q {
        let bz = dir.scale(-1.0).normalize();
        let mut by = up.sub(bz.scale(up.dot(bz)));
        if by.length_sq() < 1e-9 {
            by = V3::new(0.0, 1.0, 0.0).sub(bz.scale(bz.y));
            if by.length_sq() < 1e-9 {
                by = V3::new(1.0, 0.0, 0.0).sub(bz.scale(bz.x));
            }
        }
        by = by.normalize();
        let bx = by.cross(bz).normalize();
        Q::from_basis(bx, by, bz)
    }

    /// Solve the two-bone chain so the hand lands exactly on `target_pos`
    /// with orientation `target_quat`, elbow swung toward [`Arm::pole`].
    /// `solve(targetPos, targetQuat)`, `hands.js:999-1042`.
    ///
    /// `target_pos`/`target_quat` and [`Arm::shoulder`]/[`Arm::pole`] must
    /// all already be expressed in the same space (the arm root's parent
    /// space) — this method performs no space conversion itself. See the
    /// module doc's "pole vector" bullet.
    pub fn solve(&mut self, target_pos: V3, target_quat: Q) {
        self.hand_pos = target_pos;
        self.hand_quat = target_quat;

        let mut t = target_pos.sub(self.shoulder);
        let mut d = t.length();
        let max_d = (self.l1 + self.l2) * 0.995;
        let min_d = (self.l1 - self.l2).abs() * 1.05 + 1e-4;
        if d > max_d {
            t = t.scale(max_d / d);
            d = max_d;
        } else if d < min_d {
            t = if d < 1e-5 { V3::new(0.0, 0.0, -min_d) } else { t.scale(min_d / d) };
            d = min_d;
        }
        let dir = t.scale(1.0 / d);

        // Circle of possible elbow positions; pick the point toward the pole.
        let a = (self.l1 * self.l1 - self.l2 * self.l2 + d * d) / (2.0 * d);
        let h = (self.l1 * self.l1 - a * a).max(0.0).sqrt();
        // `hands.js:1020-1025`: the degenerate re-seed
        // (`_perp.set(side,-1,0).addScaledVector(_dir, 0)`) adds a
        // zero-scaled vector — a literal no-op preserved here only in this
        // comment, not as dead code, per the port recipe's "dead computation
        // is still part of the source" guidance; the very next line performs
        // the real projection either way.
        let mut perp = self.pole.sub(dir.scale(self.pole.dot(dir)));
        if perp.length_sq() < 1e-8 {
            let seed = V3::new(self.side, -1.0, 0.0);
            perp = seed.sub(dir.scale(seed.dot(dir)));
        }
        perp = perp.normalize();
        let elbow = self.shoulder.add(dir.scale(a)).add(perp.scale(h));

        // Upper arm: shoulder -> elbow, elbow pad (outside of the bend) on
        // the pole side.
        self.upper_pos = self.shoulder;
        let hp = elbow.sub(self.shoulder);
        if hp.length_sq() > 1e-12 {
            self.upper_quat = Self::aim_bone(hp, perp);
        }

        // Forearm: elbow -> wrist, rolled with the back of the hand.
        self.fore_pos = elbow;
        let up = V3::new(0.0, 1.0, 0.0).apply_quat(target_quat);
        let hp2 = target_pos.sub(elbow);
        if hp2.length_sq() > 1e-12 {
            self.fore_quat = Self::aim_bone(hp2, up);
        }
        self.elbow = elbow;
    }
}
