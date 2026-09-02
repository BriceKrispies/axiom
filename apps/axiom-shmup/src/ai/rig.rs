//! Ported from Claude-of-Duty `src/ai/rig.js:1-266`.
//!
//! AI — the soldier skeleton.
//!
//! 25 bones, authored in metres in the actor's bind space: feet on `y = 0`, the
//! character facing `+Z`. Because Y is up and Z is forward in a right-handed
//! frame, the character's own **right** side is at negative X (right = forward
//! x up) — every `*R` bone lives at `x < 0`.
//!
//! Bone axis convention matches what `physics`'s ragdoll expects when it adopts
//! the skeleton: local **+Y runs down the bone** toward its child and local +Z
//! points roughly forward.
//!
//! The bind pose is not a T-pose. It is a patrol carry — stock in the right
//! shoulder pocket, support hand on the handguard.
//!
//! ## What is and is not here
//!
//! `createSkeleton()` (`rig.js:243-261`) builds a fresh `THREE.Bone` hierarchy
//! plus a `THREE.Skeleton` for one actor. The bone hierarchy itself is ported —
//! it is the thing the animator drives — but it lives in
//! [`super::animator`], as the `Skeleton` arena the [`super::animator::Animator`]
//! owns, rather than here: there is no `THREE.Object3D` graph in this port to
//! hand back. `THREE.Skeleton`'s own contribution (`boneInverses`) lives there
//! too, as [`super::animator::Skeleton::bind_inverses`] — it is a pure function
//! of the bind pose this file authors, but it only ever appears multiplied by a
//! *posed* bone matrix, and those are the animator's. (The bone-matrix
//! `Float32Array` texture is not ported at all: the engine owns its own joint
//! palette behind `RunningApp::submit_skinned_draw`.) `Rig` below is exactly
//! the shared, immutable
//! half — `rig.js:117-241` — which is what `RIG` is: one instance for every
//! soldier.
//!
//! ## Precision, and Euler order
//!
//! `THREE.Vector3`/`Quaternion` store plain JS numbers, so every value here is
//! `f64`, as in the source. (There is no `Float32Array` anywhere in `rig.js` —
//! checked; the one in this slice is the animator's pose accumulator.)
//!
//! There is **no Euler anywhere in `rig.js`** — checked by grep for
//! `setFromEuler`, `.order`, `'XYZ'` and `'YXZ'`. Every rotation here is built
//! from a basis (`makeBasis` + `setFromRotationMatrix`) or composed from other
//! quaternions. The Euler-order question is live in the *sibling* files:
//! `ai/parts.js:44` and `ai/weapon.js:28` both use `'YXZ'`, while the one site
//! in this slice — `animator.js:311` — passes an explicit `'XYZ'`. They are
//! different rotations; do not share a helper between them.
//!
//! ## `const` vs `LazyLock`
//!
//! [`BORE_DIR`], [`GRIP_R`] and [`GRIP_L`] are `LazyLock<[f64; 3]>`, not
//! `const`: all three are derived through `f64::sqrt` (a `normalize`, and a
//! two-bone solve on top of it), which is not a stable `const fn`. Writing
//! them out as literal arrays would mean hand-transcribing a *computed*
//! constant, which is exactly the transcription step the port recipe bans.
//! They deref to `[f64; 3]`, so `GRIP_R[0]` and `*GRIP_R` both work; a caller
//! that needs the array by value writes `*GRIP_R`. Same precedent as
//! `crate::player::tuning::JUMP_SPEED`.

use std::collections::HashMap;
use std::sync::LazyLock;

/// `Math.hypot(a, b, c)` — V8's max-scaled, Kahan-compensated algorithm, not
/// `(a*a + b*b + c*c).sqrt()`, which rounds differently (see the port recipe's
/// "`Math.hypot` is not `sqrt(x*x+y*y+z*z)`" trap). `rig.js:239` is the one
/// `Math.hypot` in this slice. Measured against the golden's own
/// `distanceToBone` values: [`crate::jsmath::hypot3`] reproduces V8 **bit for
/// bit** across every bone/probe pair captured, where the naive form is off by
/// up to 4.4e-16.
use crate::jsmath::hypot3;
use crate::weapons::rig_math::{Q, V3};

/// `const H = 1.8` (`rig.js:22`) — the reference height the proportions are
/// authored at (8 heads). Declared in the source and never read: dead, but
/// dead computation in the source is still part of the source (see the port
/// recipe), so it is transcribed rather than dropped.
#[allow(dead_code)]
const H: f64 = 1.8;

/* Arm/leg segment lengths for the reference height. `rig.js:25-27`. */
const UPPER_ARM: f64 = 0.29;
const FOREARM: f64 = 0.255;
/// `const HAND = 0.095` (`rig.js:27`) — likewise declared and never read.
#[allow(dead_code)]
const HAND: f64 = 0.095;

/// Two-bone solve used at author time to place the elbow from the shoulder,
/// the wrist and a pole hint. `rig.js:33-47`.
fn solve_elbow(s_in: [f64; 3], w_in: [f64; 3], l1: f64, l2: f64, pole: [f64; 3]) -> V3 {
    let s = V3::from_array(s_in);
    let w = V3::from_array(w_in);
    let mut axis = w.subtract(s);
    let d = (l1 + l2 - 1e-4).min(((l1 - l2).abs() + 1e-4).max(axis.length()));
    axis = axis.normalize_or_zero();
    let a = (l1 * l1 - l2 * l2 + d * d) / (2.0 * d);
    let h = (0.0f64).max(l1 * l1 - a * a).sqrt();
    let p = V3::from_array(pole).normalize_or_zero();
    // component of the pole perpendicular to the bone axis
    let mut perp = p.add_scaled(axis, -p.dot(axis));
    if perp.length_squared() < 1e-8 {
        // `perp.set(0, -1, 0).addScaledVector(axis, -axis.y * -1)` — the
        // source's double negation, transcribed as written (`-axis.y * -1`
        // *is* `axis.y`, which is the correct perpendicular projection of
        // `(0,-1,0)`; the spelling is kept so this diffs against the source).
        perp = V3::new(0.0, -1.0, 0.0).add_scaled(axis, -axis.y * -1.0);
    }
    perp = perp.normalize_or_zero();
    s.add_scaled(axis, a).add_scaled(perp, h)
}

/* -------------------------------------------------------------------- */
/* Hand targets — derived from where the weapon sits in the bind pose.  */
/* -------------------------------------------------------------------- */

/// Bore line of the carried weapon in bind pose: origin. `rig.js:54`.
pub const BORE_ORIGIN: [f64; 3] = [-0.148, 1.398, -0.078];

/// Bore direction, `new THREE.Vector3(0.115, -0.10, 1).normalize_or_zero()`
/// (`rig.js:55-58`). A `LazyLock`, not a `const`: `f64::sqrt` is not a stable
/// `const fn`.
pub static BORE_DIR: LazyLock<[f64; 3]> = LazyLock::new(|| {
    let v = V3::new(0.115, -0.10, 1.0).normalize_or_zero();
    [v.x, v.y, v.z]
});

/// `alongBore(t, dropY)`. `rig.js:60-66`.
fn along_bore(t: f64, drop_y: f64) -> [f64; 3] {
    let d = *BORE_DIR;
    [
        BORE_ORIGIN[0] + d[0] * t,
        BORE_ORIGIN[1] + d[1] * t - drop_y,
        BORE_ORIGIN[2] + d[2] * t,
    ]
}

/// Firing hand (pistol grip) in bind pose. `rig.js:69`.
pub static GRIP_R: LazyLock<[f64; 3]> = LazyLock::new(|| along_bore(0.26, 0.095));
/// Support hand (handguard) in bind pose. `rig.js:70`.
pub static GRIP_L: LazyLock<[f64; 3]> = LazyLock::new(|| along_bore(0.45, 0.05));

const SHOULDER_R: [f64; 3] = [-0.172, 1.425, 0.004];
const SHOULDER_L: [f64; 3] = [0.172, 1.425, 0.004];

static ELBOW_R: LazyLock<V3> =
    LazyLock::new(|| solve_elbow(SHOULDER_R, *GRIP_R, UPPER_ARM, FOREARM, [-0.35, -1.0, -0.45]));
static ELBOW_L: LazyLock<V3> =
    LazyLock::new(|| solve_elbow(SHOULDER_L, *GRIP_L, UPPER_ARM, FOREARM, [0.55, -1.0, -0.2]));

/* -------------------------------------------------------------------- */
/* Bone table                                                           */
/* -------------------------------------------------------------------- */

/// One row of `BONES` (`rig.js:83-113`): `[name, parent, bind world position,
/// optional up hint, optional leaf dir]`. The JS rows are ragged arrays read
/// positionally (`spec[2]`, `spec[3]`, `spec[4]`); the named fields here are
/// the same five slots.
#[derive(Debug, Clone, Copy)]
pub struct BoneSpec {
    pub name: &'static str,
    pub parent: Option<&'static str>,
    /// `spec[2]` — `None` for a leaf hung off its parent by `leaf_dir`.
    pub pos: Option<[f64; 3]>,
    /// `spec[3]` — the up hint for the bone's basis; defaults to `[0, 0, 1]`.
    pub up: Option<[f64; 3]>,
    /// `spec[4]` — an explicit leaf direction.
    pub leaf_dir: Option<[f64; 3]>,
}

const fn bone(name: &'static str, parent: Option<&'static str>, pos: [f64; 3]) -> BoneSpec {
    BoneSpec { name, parent, pos: Some(pos), up: None, leaf_dir: None }
}

/// `export const BONES`. `rig.js:83-113`.
///
/// **The order is load-bearing.** Every bone is addressed by its index into
/// this table — the animator's pose accumulator, the hitbox capsules and the
/// ragdoll hand-off all index it — so this is exactly the source's order, row
/// for row (see the port recipe's "an enum used as a table index is
/// order-dependent" trap).
pub static BONES: LazyLock<Vec<BoneSpec>> = LazyLock::new(|| {
    let elbow_r = *ELBOW_R;
    let elbow_l = *ELBOW_L;
    vec![
        bone("Hips", None, [0.0, 0.98, -0.005]),
        bone("Spine", Some("Hips"), [0.0, 1.09, -0.012]),
        bone("Spine1", Some("Spine"), [0.0, 1.215, 0.0]),
        bone("Spine2", Some("Spine1"), [0.0, 1.345, 0.006]),
        bone("Neck", Some("Spine2"), [0.0, 1.475, -0.008]),
        bone("Head", Some("Neck"), [0.0, 1.552, 0.004]),
        bone("HeadTop", Some("Head"), [0.0, 1.8, 0.012]),
        //
        bone("ClavicleR", Some("Spine2"), [-0.038, 1.408, 0.016]),
        bone("UpperArmR", Some("ClavicleR"), SHOULDER_R),
        bone("ForearmR", Some("UpperArmR"), [elbow_r.x, elbow_r.y, elbow_r.z]),
        bone("HandR", Some("ForearmR"), *GRIP_R),
        BoneSpec {
            name: "FingersR",
            parent: Some("HandR"),
            pos: None,
            up: None,
            leaf_dir: Some([0.30, -0.35, 0.89]),
        },
        //
        bone("ClavicleL", Some("Spine2"), [0.038, 1.408, 0.016]),
        bone("UpperArmL", Some("ClavicleL"), SHOULDER_L),
        bone("ForearmL", Some("UpperArmL"), [elbow_l.x, elbow_l.y, elbow_l.z]),
        bone("HandL", Some("ForearmL"), *GRIP_L),
        BoneSpec {
            name: "FingersL",
            parent: Some("HandL"),
            pos: None,
            up: None,
            leaf_dir: Some([-0.32, -0.30, 0.90]),
        },
        //
        bone("UpLegR", Some("Hips"), [-0.092, 0.945, 0.0]),
        bone("LegR", Some("UpLegR"), [-0.098, 0.505, 0.02]),
        BoneSpec {
            name: "FootR",
            parent: Some("LegR"),
            pos: Some([-0.103, 0.088, -0.022]),
            up: Some([0.0, 1.0, 0.0]),
            leaf_dir: None,
        },
        BoneSpec {
            name: "ToeR",
            parent: Some("FootR"),
            pos: Some([-0.103, 0.03, 0.108]),
            up: Some([0.0, 1.0, 0.0]),
            leaf_dir: None,
        },
        //
        bone("UpLegL", Some("Hips"), [0.092, 0.945, 0.0]),
        bone("LegL", Some("UpLegL"), [0.098, 0.505, 0.02]),
        BoneSpec {
            name: "FootL",
            parent: Some("LegL"),
            pos: Some([0.103, 0.088, -0.022]),
            up: Some([0.0, 1.0, 0.0]),
            leaf_dir: None,
        },
        BoneSpec {
            name: "ToeL",
            parent: Some("FootL"),
            pos: Some([0.103, 0.03, 0.108]),
            up: Some([0.0, 1.0, 0.0]),
            leaf_dir: None,
        },
    ]
});

const LEAF_STUB: f64 = 0.075;

/// `class Rig`. `rig.js:117-262`.
pub struct Rig {
    pub names: Vec<&'static str>,
    /// `-1` for the root, exactly as the source stores it.
    pub parent: Vec<i32>,
    pub children: Vec<Vec<usize>>,
    /// World (actor) space.
    pub bind_pos: Vec<V3>,
    /// World.
    pub bind_quat: Vec<Q>,
    /// Parent space.
    pub local_pos: Vec<V3>,
    /// Parent space.
    pub local_quat: Vec<Q>,
    /// End of the bone (child or stub).
    pub tail: Vec<V3>,
    pub length: Vec<f64>,
    pub count: usize,
    pub eye_height: f64,
    map: HashMap<&'static str, usize>,
}

impl Default for Rig {
    fn default() -> Self {
        Rig::new()
    }
}

impl Rig {
    /// `constructor()`. `rig.js:118-219`.
    pub fn new() -> Self {
        let bones = &*BONES;
        let n = bones.len();

        let mut names: Vec<&'static str> = Vec::with_capacity(n);
        let mut map: HashMap<&'static str, usize> = HashMap::with_capacity(n);
        let mut parent: Vec<i32> = Vec::with_capacity(n);
        let mut children: Vec<Vec<usize>> = Vec::with_capacity(n);

        for (i, spec) in bones.iter().enumerate() {
            names.push(spec.name);
            map.insert(spec.name, i);
            // The source pushes -1/-2 here and resolves in the next loop; the
            // placeholder is never read, so only the resolved value matters.
            parent.push(if spec.parent.is_none() { -1 } else { -2 });
            children.push(Vec::new());
        }
        for (i, spec) in bones.iter().enumerate() {
            let pi = match spec.parent {
                None => -1i32,
                Some(p) => map[p] as i32,
            };
            parent[i] = pi;
            if pi >= 0 {
                children[pi as usize].push(i);
            }
        }

        // ---- positions -----------------------------------------------------
        let mut bind_pos: Vec<V3> = Vec::with_capacity(n);
        for (i, spec) in bones.iter().enumerate() {
            let p = match spec.pos {
                Some(p) => p,
                None => {
                    // leaf with an explicit direction: hang it off the parent
                    let pi = parent[i] as usize;
                    let base = bind_pos[pi];
                    let d = V3::from_array(spec.leaf_dir.expect("leaf spec has spec[4]")).normalize_or_zero();
                    [
                        base.x + d.x * LEAF_STUB,
                        base.y + d.y * LEAF_STUB,
                        base.z + d.z * LEAF_STUB,
                    ]
                }
            };
            bind_pos.push(V3::from_array(p));
        }

        // ---- world rotations: +Y down the bone -----------------------------
        let mut tail_v: Vec<V3> = Vec::with_capacity(n);
        let mut length: Vec<f64> = Vec::with_capacity(n);
        let mut bind_quat: Vec<Q> = Vec::with_capacity(n);
        for (i, spec) in bones.iter().enumerate() {
            let kids = &children[i];
            let mut tail;
            if !kids.is_empty() {
                // primary child: the first one listed (chains are authored in order)
                tail = bind_pos[kids[0]];
                if kids.len() > 1 {
                    // a branch point (Hips, Spine2): aim at the average of the chain kids
                    let primary = kids.iter().copied().find(|k| !is_clavicle_or_upleg(names[*k]));
                    match primary {
                        Some(k) => tail = bind_pos[k],
                        None => {
                            // Unreachable for the authored table (Hips has
                            // `Spine`, Spine2 has `Neck`), but the source's
                            // fallback is transcribed rather than dropped.
                            let mut acc = V3::ZERO;
                            for k in kids {
                                acc = acc.add(bind_pos[*k]);
                            }
                            tail = acc.mul_scalar(1.0 / kids.len() as f64);
                        }
                    }
                }
            } else {
                let d = match spec.leaf_dir {
                    Some(ld) => V3::from_array(ld).normalize_or_zero(),
                    None => bind_pos[i].subtract(bind_pos[parent[i] as usize]).normalize_or_zero(),
                };
                tail = bind_pos[i].add_scaled(d, LEAF_STUB);
            }
            tail_v.push(tail);

            let mut y_axis = tail.subtract(bind_pos[i]);
            length.push(y_axis.length());
            if y_axis.length_squared() < 1e-10 {
                y_axis = V3::new(0.0, 1.0, 0.0);
            }
            y_axis = y_axis.normalize_or_zero();
            let hint = spec.up.unwrap_or([0.0, 0.0, 1.0]);
            let mut up = V3::from_array(hint);
            if up.dot(y_axis).abs() > 0.985 {
                up = V3::new(1.0, 0.0, 0.0);
            }
            let x_axis = y_axis.cross(up).normalize_or_zero();
            let z_axis = x_axis.cross(y_axis).normalize_or_zero();
            // `m.makeBasis(x, y, z)` then `setFromRotationMatrix(m)`: the basis
            // vectors are the matrix's **columns** (THREE's `elements` are
            // column-major) — see the port recipe's matrix-storage-order trap.
            // `Q::from_basis` takes the three columns in exactly that role.
            bind_quat.push(Q::from_basis(x_axis, y_axis, z_axis));
        }

        // ---- local transforms ---------------------------------------------
        let mut local_pos: Vec<V3> = Vec::with_capacity(n);
        let mut local_quat: Vec<Q> = Vec::with_capacity(n);
        for i in 0..n {
            let pi = parent[i];
            if pi < 0 {
                local_pos.push(bind_pos[i]);
                local_quat.push(bind_quat[i]);
            } else {
                let inv = bind_quat[pi as usize].invert();
                let v = inv.rotate(bind_pos[i].subtract(bind_pos[pi as usize]));
                local_pos.push(v);
                local_quat.push(inv.multiply(bind_quat[i]));
            }
        }

        Rig {
            names,
            parent,
            children,
            bind_pos,
            bind_quat,
            local_pos,
            local_quat,
            tail: tail_v,
            length,
            count: n,
            eye_height: 1.665,
            map,
        }
    }

    /// `index(name)`. `rig.js:221-225` — the source throws on an unknown bone;
    /// every call site in the port passes a table name, so this panics with
    /// the same message rather than returning a `Result` nothing would check.
    pub fn index(&self, name: &str) -> usize {
        match self.map.get(name) {
            Some(i) => *i,
            None => panic!("[ai] unknown bone \"{name}\""),
        }
    }

    /// `has(name)`. `rig.js:227-229`.
    pub fn has(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    /// `RIG.bindPos[RIG.index(name)]` as a plain array — the shape the
    /// callers that place geometry on the bind pose (`soldier.js`,
    /// `parts.js`) want, without each of them reaching through
    /// [`Rig::bind_pos`]'s `V3`. Not a widening of the surface: it is the
    /// same two calls the source makes at every such site, spelled once.
    pub fn bind_pos_of(&self, name: &str) -> [f64; 3] {
        let p = self.bind_pos[self.index(name)];
        [p.x, p.y, p.z]
    }

    /// `RIG.bindPos[i]` as a plain array.
    pub fn bind_pos_at(&self, i: usize) -> [f64; 3] {
        let p = self.bind_pos[i];
        [p.x, p.y, p.z]
    }

    /// Distance from a point to a bone's bind-pose segment. `rig.js:232-240`.
    pub fn distance_to_bone(&self, i: usize, x: f64, y: f64, z: f64) -> f64 {
        let a = self.bind_pos[i];
        let b = self.tail[i];
        let (dx, dy, dz) = (b.x - a.x, b.y - a.y, b.z - a.z);
        let l2 = dx * dx + dy * dy + dz * dz;
        let mut t = if l2 > 1e-12 {
            ((x - a.x) * dx + (y - a.y) * dy + (z - a.z) * dz) / l2
        } else {
            0.0
        };
        if t < 0.0 {
            t = 0.0;
        } else if t > 1.0 {
            t = 1.0;
        }
        hypot3(x - (a.x + dx * t), y - (a.y + dy * t), z - (a.z + dz * t))
    }
}

/// `!/Clavicle|UpLeg/.test(name)`, inverted. `rig.js:172`.
fn is_clavicle_or_upleg(name: &str) -> bool {
    name.contains("Clavicle") || name.contains("UpLeg")
}

/// `export const RIG = new Rig()` (`rig.js:265`) — one shared rig; the bind
/// pose is identical for every soldier.
pub static RIG: LazyLock<Rig> = LazyLock::new(Rig::new);
