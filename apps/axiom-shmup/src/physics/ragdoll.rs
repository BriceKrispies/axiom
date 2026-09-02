//! Ported from Claude-of-Duty `src/physics/ragdoll.js:1-763` — the whole file
//! except the three `THREE.Skeleton` methods (see "Not ported" below).
//!
//! The source's own header explains the shape: "an articulated chain of
//! capsules solved with position-based dynamics (Gauss-Seidel, a handful of
//! iterations per fixed step) … Each bone is a segment of two particles;
//! joints are shared particles, so joint separation is impossible by
//! construction and only the *angular* limits need constraints."
//!
//! Constraints, applied in order every iteration:
//!   1. bone length      (hard distance, stiffness 1)
//!   2. cone limit       (swing of a bone relative to its parent)
//!   3. twist limit      (roll of a bone's reference frame, damped)
//!   4. world contact    (capsule vs static BVH + Coulomb friction)
//!
//! # Storage width is part of the algorithm
//!
//! `ragdoll.js` mixes `Float32Array` and `Float64Array` deliberately, and the
//! solver reads back what it stored. Every field below carries the source's
//! width exactly:
//!
//! | source                                                        | width |
//! |---------------------------------------------------------------|-------|
//! | `boneHead` / `boneTail` / `boneParent` / `selfPairs`            | `i32` |
//! | `boneLen` `boneRadius` `boneMass` `boneCone` `boneTwist` `boneUp` | `f32` |
//! | `px py pz qx qy qz invMass`                                     | `f64` |
//! | `aabb` (a plain object, not a typed array)                      | `f64` |
//!
//! This is not cosmetic. Measured on the `standing_drop_on_floor` golden
//! scenario, holding `boneLen` in `f64` instead of `f32` moves particle
//! positions by 8.6e-9 after one step and 2.7e-6 after 300; holding
//! `boneRadius` in `f64` moves them by 8.8e-7. Both are far above the
//! trajectory's real noise floor (see the tolerance note below), so an
//! all-`f64` port fails the golden — which is the point. Arithmetic is
//! performed in `f64` (a JavaScript number *is* an `f64`) and rounded on
//! store, exactly as a `Float32Array` write does.
//!
//! # `Math.hypot` is not `sqrt(x*x + y*y + z*z)`
//!
//! `ragdoll.js` calls `Math.hypot` at eighteen sites, most of them on the
//! per-iteration hot path. V8
//! implements it (the `MathHypot` Torque builtin) by scaling every argument
//! by the largest magnitude and summing the squares with Kahan compensation —
//! it disagrees with the naive form on ~41% of random triples (measured:
//! 205,887 of 500,000 by the capture script). [`hypot3`] is that builtin
//! transcribed, and the capture script validates the transcription against the
//! real `Math.hypot` over 500,000 triples before it writes a byte (0
//! mismatches), then pins it in the golden so the Rust side is checked too.
//!
//! # Not ported
//!
//! `adoptSkeleton` (`:653-663`), `writeToSkeleton` (`:666-695`) and
//! `specFromSkeleton` (`:709-763`) all traffic in a live `THREE.Skeleton` /
//! `THREE.Bone` object graph — parent pointers, `matrixWorld`,
//! `updateWorldMatrix`, `decompose`. There is no such graph in this port and
//! inventing one here would be a scene-graph abstraction wedged into a
//! physics module. [`Ragdoll::get_bone_transform`] and
//! [`Ragdoll::get_bone_capsule`] are the readback the renderer actually needs;
//! whichever tier grows a skinned-mesh binding owns the write-back. Same
//! precedent as `bvh.rs` omitting `bakeMesh`.
//!
//! # Source quirk: the humanoid rig is five disconnected pieces
//!
//! The module header claims "joints are shared particles, so joint separation
//! is impossible by construction". That is true of the *chains*, but
//! [`humanoid_spec`]'s own coordinates defeat it at every limb root:
//! `upperArmL.head` is `(-sh, 0.815h, 0)` while its parent `chest.tail` is
//! `(0, 0.83h, 0)`, and `thighL.head` is `(-hip, 0.53h, 0)` while its parent
//! `pelvis.head` is `(0, 0.53h, 0)`. Neither pair rounds to the same
//! millimetre, so neither shares a particle. The 15-bone humanoid therefore
//! resolves to **20 particles in five disconnected islands** — spine+head,
//! each arm, each leg — coupled only by the *cone* constraint, which
//! constrains direction and translates a limb bodily rather than pinning it.
//! The limbs consequently drift apart: a doll dropped 35 cm onto a flat floor
//! settles as a 4-metre-wide smear. This is ported faithfully and pinned by
//! `tests/physics_ragdoll_port.rs`; fixing it means changing the rig
//! coordinates, which would change every captured trajectory.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::physics::bvh::{Aabb, StaticWorld};
use crate::physics::math::closest_pt_seg_seg;
use crate::physics::surfaces::mask;

/// `ragdoll.js:26`.
pub const DEG: f64 = std::f64::consts::PI / 180.0;

/// Metres per fixed step, anti-explosion clamp. `ragdoll.js:73`.
pub const MAX_PARTICLE_STEP: f64 = 0.35;
/// `ragdoll.js:74`.
pub const SLEEP_MOTION: f64 = 0.0022;
/// `ragdoll.js:75`.
pub const SLEEP_TIME: f64 = 0.6;

thread_local! {
    /// `ragdoll.js:77` (`let _nextRagdollId = 1`). The source's module-level
    /// counter. A `thread_local` rather than a process-wide atomic because
    /// `cargo test` runs test functions on many threads and a shared counter
    /// would make the id depend on scheduling; per-thread it is deterministic
    /// in the same way the single-threaded source is. Nothing in the solver
    /// reads the id.
    static NEXT_RAGDOLL_ID: Cell<i32> = const { Cell::new(1) };
}

/// V8's `Math.hypot`, scaled and Kahan-compensated.
///
/// This file carried its own transcription of it; every other caller in the app
/// already reaches `axiom_math::hypot3` through [`crate::jsmath`], so this was
/// the last private copy. Same algorithm, one implementation.
pub use axiom_math::hypot3;

/// `Math.round` — rounds half towards `+Infinity`, which is **not**
/// [`f64::round`] (half away from zero). `Math.round(-2.5)` is `-2`;
/// `(-2.5f64).round()` is `-3.0`. Used only by the constructor's
/// millimetre particle-merge key, where a `-0.5` boundary decides whether two
/// bone endpoints become one particle.
///
/// Written as `floor(x); x - floor >= 0.5` rather than `floor(x + 0.5)`
/// because the latter is wrong for `0.49999999999999994` (adding `0.5` rounds
/// it up to exactly `1.0`), and ECMA-262 specifies the former behaviour.
use crate::jsmath::round as js_round;

/// `addPoint` (`ragdoll.js:115-126`), lifted out of the constructor because
/// Rust closures cannot hold `&mut` to four sibling `Vec`s and a `HashMap` as
/// conveniently as a JS closure holds four arrays.
///
/// Transforms one spec point into world space, then returns the index of the
/// particle at that millimetre cell — creating it on first sight. This
/// millimetre snap is what welds a bone's tail to its child's head, and what
/// *fails* to weld an arm to the chest (see the module doc comment).
fn add_point(
    arr: &[f64; 3],
    mat: Option<&[f64; 16]>,
    map: &mut HashMap<(i64, i64, i64), usize>,
    px: &mut Vec<f64>,
    py: &mut Vec<f64>,
    pz: &mut Vec<f64>,
    pm: &mut Vec<f64>,
) -> usize {
    let (mut vx, mut vy, mut vz) = (arr[0], arr[1], arr[2]);
    if let Some(e) = mat {
        // `THREE.Vector3.applyMatrix4`, verbatim: column-major `elements`,
        // including the perspective divide the source does not skip.
        let (x, y, z) = (vx, vy, vz);
        let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
        vx = (e[0] * x + e[4] * y + e[8] * z + e[12]) * w;
        vy = (e[1] * x + e[5] * y + e[9] * z + e[13]) * w;
        vz = (e[2] * x + e[6] * y + e[10] * z + e[14]) * w;
    }
    // `key(x, y, z)` (`ragdoll.js:110-111`) builds a `"mx,my,mz"` string from
    // `Math.round(v * 1000)`. A 3-tuple of the same rounded integers is the
    // same equivalence relation without the string formatting; `-0` and `0`
    // collapse together here exactly as they do in the template literal.
    let k = (
        js_round(vx * 1000.0) as i64,
        js_round(vy * 1000.0) as i64,
        js_round(vz * 1000.0) as i64,
    );
    match map.get(&k) {
        Some(&i) => i,
        None => {
            let i = px.len();
            px.push(vx);
            py.push(vy);
            pz.push(vz);
            pm.push(0.0);
            map.insert(k, i);
            i
        }
    }
}

/* ------------------------------------------------------------------ */
/* Bone spec                                                           */
/* ------------------------------------------------------------------ */

/// One entry of the bone spec array. `ragdoll.js:40-49` (`b(...)`) builds
/// these for the humanoid; a caller may hand in its own.
///
/// `radius`/`mass`/`parent`/`cone`/`twist` are `Option` because the
/// constructor reads them with `??` (`ragdoll.js:145-149`) and substitutes
/// `0.06` / `4` / `-1` / `70 * DEG` / `40 * DEG`. `??` is null-coalescing, not
/// truthiness — `parent: Some(0)` stays `0` and is *not* replaced by `-1`.
#[derive(Debug, Clone, PartialEq)]
pub struct BoneSpec {
    pub name: String,
    pub head: [f64; 3],
    pub tail: [f64; 3],
    pub radius: Option<f64>,
    pub mass: Option<f64>,
    pub parent: Option<i32>,
    pub cone: Option<f64>,
    pub twist: Option<f64>,
}

impl BoneSpec {
    /// A spec entry with every optional field absent, so the constructor's
    /// defaults apply. `head`/`tail` have no default in the source either.
    pub fn new(name: &str, head: [f64; 3], tail: [f64; 3]) -> Self {
        BoneSpec {
            name: name.to_string(),
            head,
            tail,
            radius: None,
            mass: None,
            parent: None,
            cone: None,
            twist: None,
        }
    }
}

/// A 15-capsule humanoid sized from a total height, in the actor's local
/// frame (feet at `y = 0`, `+Z` forward). Proportions are the standard
/// 7.5-head figure. `ragdoll.js:36-69`.
///
/// The source's defaults are `height = 1.8`, `scaleMass = 82`.
pub fn humanoid_spec(height: f64, scale_mass: f64) -> Vec<BoneSpec> {
    let h = height;
    let m_scale = scale_mass;
    let y = |f: f64| h * f;
    // `b(...)` (`ragdoll.js:40-49`).
    let b = |name: &str,
             hx: f64,
             hy: f64,
             hz: f64,
             tx: f64,
             ty: f64,
             tz: f64,
             r: f64,
             m: f64,
             parent: i32,
             cone: f64,
             twist: f64| BoneSpec {
        name: name.to_string(),
        head: [hx, hy, hz],
        tail: [tx, ty, tz],
        radius: Some(r * h),
        mass: Some(m * m_scale),
        parent: Some(parent),
        cone: Some(cone * DEG),
        twist: Some(twist * DEG),
    };
    let sh = h * 0.105; // half shoulder width
    let hip = h * 0.055;
    vec![
        /* 0 */ b("pelvis", 0.0, y(0.53), 0.0, 0.0, y(0.63), 0.0, 0.085, 0.14, -1, 0.0, 0.0),
        /* 1 */ b("spine", 0.0, y(0.63), 0.0, 0.0, y(0.74), 0.0, 0.082, 0.12, 0, 22.0, 18.0),
        /* 2 */ b("chest", 0.0, y(0.74), 0.0, 0.0, y(0.83), 0.0, 0.088, 0.19, 1, 20.0, 15.0),
        /* 3 */ b("neck", 0.0, y(0.83), 0.0, 0.0, y(0.875), 0.0, 0.042, 0.02, 2, 30.0, 25.0),
        /* 4 */ b("head", 0.0, y(0.875), 0.0, 0.0, y(0.97), 0.01, 0.062, 0.07, 3, 42.0, 30.0),
        /* 5 */
        b("upperArmL", -sh, y(0.815), 0.0, -sh - h * 0.015, y(0.65), 0.0, 0.045, 0.027, 2, 85.0, 60.0),
        /* 6 */
        b("forearmL", -sh - h * 0.015, y(0.65), 0.0, -sh - h * 0.02, y(0.50), 0.0, 0.037, 0.018, 5, 80.0, 45.0),
        /* 7 */
        b("handL", -sh - h * 0.02, y(0.50), 0.0, -sh - h * 0.02, y(0.44), 0.0, 0.032, 0.006, 6, 55.0, 40.0),
        /* 8 */
        b("upperArmR", sh, y(0.815), 0.0, sh + h * 0.015, y(0.65), 0.0, 0.045, 0.027, 2, 85.0, 60.0),
        /* 9 */
        b("forearmR", sh + h * 0.015, y(0.65), 0.0, sh + h * 0.02, y(0.50), 0.0, 0.037, 0.018, 8, 80.0, 45.0),
        /*10 */
        b("handR", sh + h * 0.02, y(0.50), 0.0, sh + h * 0.02, y(0.44), 0.0, 0.032, 0.006, 9, 55.0, 40.0),
        /*11 */
        b("thighL", -hip, y(0.53), 0.0, -hip * 1.05, y(0.29), 0.0, 0.062, 0.10, 0, 75.0, 35.0),
        /*12 */
        b("shinL", -hip * 1.05, y(0.29), 0.0, -hip * 1.05, y(0.055), 0.0, 0.048, 0.045, 11, 70.0, 20.0),
        /*13 */
        b("thighR", hip, y(0.53), 0.0, hip * 1.05, y(0.29), 0.0, 0.062, 0.10, 0, 75.0, 35.0),
        /*14 */
        b("shinR", hip * 1.05, y(0.29), 0.0, hip * 1.05, y(0.055), 0.0, 0.048, 0.045, 13, 70.0, 20.0),
    ]
}

/* ------------------------------------------------------------------ */
/* Construction options                                                */
/* ------------------------------------------------------------------ */

/// `new Ragdoll(world, opts)`'s option bag (`ragdoll.js:88-103`). Every field
/// is `Option` so that "absent" and "explicitly set to the default value" stay
/// distinguishable, exactly as `??` does in the source.
///
/// The source's `userData` and `actor` (`ragdoll.js:96-97`) are dropped: they
/// are opaque back-pointers to the `ai` actor that owns the corpse, read by
/// nothing in this file. Whichever tier owns actor lifetimes carries that
/// association.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RagdollOpts {
    /// `opts.bones` — a caller-supplied spec. When absent, `humanoidSpec` is
    /// built from `height`/`mass`.
    pub bones: Option<Vec<BoneSpec>>,
    /// `opts.height`, default `1.8`. Ignored when `bones` is present.
    pub height: Option<f64>,
    /// `opts.mass`, default `82`. Ignored when `bones` is present.
    pub mass: Option<f64>,
    /// `opts.transform` — a `THREE.Matrix4` placing the spec into world
    /// space, as its 16 `.elements` in **column-major** order (the same
    /// convention `math::ray_obb` already takes). `Vector3.applyMatrix4`
    /// reads nothing else.
    pub transform: Option<[f64; 16]>,
    /// `opts.gravity`, m/s^2 (negative). Default `-20.6`.
    pub gravity: Option<f64>,
    /// `opts.iterations` — Gauss-Seidel iterations per fixed step. Default 6.
    pub iterations: Option<u32>,
    /// `opts.mask`. Default `MASK.DEBRIS`.
    pub mask: Option<u16>,
    /// `opts.damping`. Default `0.985`.
    pub damping: Option<f64>,
    /// `opts.friction`. Default `0.72`.
    pub friction: Option<f64>,
}

/// World-space capsule of one bone. `getBoneCapsule`'s `out` record
/// (`ragdoll.js:609-615`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BoneCapsule {
    pub ax: f64,
    pub ay: f64,
    pub az: f64,
    pub bx: f64,
    pub by: f64,
    pub bz: f64,
    pub r: f64,
}

/// World transform of one bone. `getBoneTransform`'s two out-parameters
/// (`ragdoll.js:621-647`), returned by value. `quat` is `[x, y, z, w]` — the
/// component order `THREE.Quaternion` uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoneTransform {
    pub pos: [f64; 3],
    pub quat: [f64; 4],
}

/* ------------------------------------------------------------------ */
/* Ragdoll                                                             */
/* ------------------------------------------------------------------ */

/// `ragdoll.js:79-702` (`class Ragdoll`).
pub struct Ragdoll {
    pub id: i32,
    /// `Option` because the source accepts a null world and
    /// `_solveContacts` early-returns on it (`ragdoll.js:424`).
    world: Option<Rc<StaticWorld>>,
    pub gravity: f64,
    pub iterations: u32,
    pub mask: u16,
    pub linear_damping: f64,
    pub friction: f64,
    pub alive: bool,
    pub sleeping: bool,
    pub sleep_timer: f64,
    pub age: f64,

    pub spec: Vec<BoneSpec>,
    pub bone_count: usize,

    /// `Int32Array`.
    pub bone_head: Vec<i32>,
    /// `Int32Array`.
    pub bone_tail: Vec<i32>,
    /// `Float32Array`.
    pub bone_len: Vec<f32>,
    /// `Float32Array`.
    pub bone_radius: Vec<f32>,
    /// `Float32Array`.
    pub bone_mass: Vec<f32>,
    /// `Int32Array`.
    pub bone_parent: Vec<i32>,
    /// `Float32Array`.
    pub bone_cone: Vec<f32>,
    /// `Float32Array`.
    pub bone_twist: Vec<f32>,
    /// Reference up-vector per bone, parallel-transported for twist.
    /// `Float32Array`, `bone_count * 3`.
    pub bone_up: Vec<f32>,

    pub particle_count: usize,
    /// `Float64Array` — current position.
    pub px: Vec<f64>,
    pub py: Vec<f64>,
    pub pz: Vec<f64>,
    /// `Float64Array` — previous position (Verlet).
    pub qx: Vec<f64>,
    pub qy: Vec<f64>,
    pub qz: Vec<f64>,
    /// `Float64Array`.
    pub inv_mass: Vec<f64>,

    /// A plain object in the source, so `f64` throughout — same reasoning as
    /// `bvh::StaticWorld::aabb`.
    pub aabb: Aabb,
    /// `Int32Array`, flat `[i, j, i, j, ...]`.
    pub self_pairs: Vec<i32>,
}

impl Ragdoll {
    /// `ragdoll.js:88-191` (the constructor).
    pub fn new(world: Option<Rc<StaticWorld>>, opts: RagdollOpts) -> Self {
        let id = NEXT_RAGDOLL_ID.with(|c| {
            let v = c.get();
            c.set(v + 1);
            v
        });

        let spec = opts
            .bones
            .clone()
            .unwrap_or_else(|| humanoid_spec(opts.height.unwrap_or(1.8), opts.mass.unwrap_or(82.0)));
        let nb = spec.len();

        // --- particle set with shared joints ---
        let mut px: Vec<f64> = Vec::new();
        let mut py: Vec<f64> = Vec::new();
        let mut pz: Vec<f64> = Vec::new();
        let mut pm: Vec<f64> = Vec::new();
        let mut map: HashMap<(i64, i64, i64), usize> = HashMap::new();
        let mat = opts.transform;

        let mut bone_head = vec![0_i32; nb];
        let mut bone_tail = vec![0_i32; nb];
        let bone_len = vec![0.0_f32; nb];
        let mut bone_radius = vec![0.0_f32; nb];
        let mut bone_mass = vec![0.0_f32; nb];
        let mut bone_parent = vec![0_i32; nb];
        let mut bone_cone = vec![0.0_f32; nb];
        let mut bone_twist = vec![0.0_f32; nb];
        let mut bone_up = vec![0.0_f32; nb * 3];

        for i in 0..nb {
            let s = &spec[i];
            let a = add_point(&s.head, mat.as_ref(), &mut map, &mut px, &mut py, &mut pz, &mut pm);
            let c = add_point(&s.tail, mat.as_ref(), &mut map, &mut px, &mut py, &mut pz, &mut pm);
            bone_head[i] = a as i32;
            bone_tail[i] = c as i32;
            bone_radius[i] = s.radius.unwrap_or(0.06) as f32;
            bone_mass[i] = s.mass.unwrap_or(4.0) as f32;
            bone_parent[i] = s.parent.unwrap_or(-1);
            bone_cone[i] = s.cone.unwrap_or(70.0 * DEG) as f32;
            bone_twist[i] = s.twist.unwrap_or(40.0 * DEG) as f32;
            // `pm` is a plain JS array (f64) but `boneMass` was just rounded
            // to f32 on store, so this reads the rounded value back.
            pm[a] += bone_mass[i] as f64 * 0.5;
            pm[c] += bone_mass[i] as f64 * 0.5;
            // Dead in the source — `_initUp(i)` below overwrites all three
            // components before anything reads them. Kept because the
            // judgement that it is dead can be wrong and preserving it costs
            // nothing.
            bone_up[i * 3] = 0.0;
            bone_up[i * 3 + 1] = 0.0;
            bone_up[i * 3 + 2] = 1.0;
        }

        let np = px.len();
        let qx = px.clone();
        let qy = py.clone();
        let qz = pz.clone();
        let mut inv_mass = vec![0.0_f64; np];
        for i in 0..np {
            inv_mass[i] = if pm[i] > 0.0 { 1.0 / pm[i] } else { 0.0 };
        }

        let mut r = Ragdoll {
            id,
            world,
            gravity: opts.gravity.unwrap_or(-20.6),
            iterations: opts.iterations.unwrap_or(6),
            mask: opts.mask.unwrap_or(mask::DEBRIS),
            linear_damping: opts.damping.unwrap_or(0.985),
            friction: opts.friction.unwrap_or(0.72),
            alive: true,
            sleeping: false,
            sleep_timer: 0.0,
            age: 0.0,
            spec,
            bone_count: nb,
            bone_head,
            bone_tail,
            bone_len,
            bone_radius,
            bone_mass,
            bone_parent,
            bone_cone,
            bone_twist,
            bone_up,
            particle_count: np,
            px,
            py,
            pz,
            qx,
            qy,
            qz,
            inv_mass,
            aabb: Aabb::default(),
            self_pairs: Vec::new(),
        };

        for i in 0..nb {
            let a = r.bone_head[i] as usize;
            let c = r.bone_tail[i] as usize;
            r.bone_len[i] =
                hypot3(r.px[c] - r.px[a], r.py[c] - r.py[a], r.pz[c] - r.pz[a]) as f32;
            // The comparison reads the f32-rounded length back, and the
            // replacement is `1e-4` narrowed on store.
            if (r.bone_len[i] as f64) < 1e-4 {
                r.bone_len[i] = 1e-4_f64 as f32;
            }
            r.init_up(i);
        }

        r.update_aabb();
        let pairs = r.build_self_pairs();
        r.self_pairs = pairs;
        r
    }

    /// Bone pairs worth testing against each other. `ragdoll.js:199-217`.
    ///
    /// Bones that share a joint are excluded (they always touch), and so is
    /// any pair that already overlaps in the bind pose — pelvis/thigh,
    /// chest/upper-arm — otherwise the solver would spend every step fighting
    /// the rig itself and the doll would inflate.
    fn build_self_pairs(&self) -> Vec<i32> {
        let mut pairs: Vec<i32> = Vec::new();
        for i in 0..self.bone_count {
            for j in (i + 1)..self.bone_count {
                let ai = self.bone_head[i] as usize;
                let bi = self.bone_tail[i] as usize;
                let aj = self.bone_head[j] as usize;
                let bj = self.bone_tail[j] as usize;
                if ai == aj || ai == bj || bi == aj || bi == bj {
                    continue;
                }
                let rad = self.bone_radius[i] as f64 + self.bone_radius[j] as f64;
                let ss = closest_pt_seg_seg(
                    self.px[ai], self.py[ai], self.pz[ai],
                    self.px[bi], self.py[bi], self.pz[bi],
                    self.px[aj], self.py[aj], self.pz[aj],
                    self.px[bj], self.py[bj], self.pz[bj],
                );
                if ss.d2 < rad * rad * 0.95 {
                    continue;
                }
                pairs.push(i as i32);
                pairs.push(j as i32);
            }
        }
        pairs
    }

    /// `ragdoll.js:219-233`.
    fn init_up(&mut self, i: usize) {
        let a = self.bone_head[i] as usize;
        let c = self.bone_tail[i] as usize;
        let mut dx = self.px[c] - self.px[a];
        let mut dy = self.py[c] - self.py[a];
        let mut dz = self.pz[c] - self.pz[a];
        // `Math.hypot(...) || 1` — JS `||` falls through on `0`, `NaN` and
        // `-0`. `NaN` is not possible here without a NaN particle, but the
        // zero case is exactly what the guard is for.
        let l0 = hypot3(dx, dy, dz);
        let l = if l0 == 0.0 || l0.is_nan() { 1.0 } else { l0 };
        dx /= l;
        dy /= l;
        dz /= l;
        // pick any axis not parallel to the bone
        let (mut ux, mut uy, mut uz) = (0.0, 0.0, 1.0);
        if dz.abs() > 0.9 {
            ux = 1.0;
            uy = 0.0;
            uz = 0.0;
        }
        let d = ux * dx + uy * dy + uz * dz;
        ux -= dx * d;
        uy -= dy * d;
        uz -= dz * d;
        let ul0 = hypot3(ux, uy, uz);
        let ul = if ul0 == 0.0 || ul0.is_nan() { 1.0 } else { ul0 };
        self.bone_up[i * 3] = (ux / ul) as f32;
        self.bone_up[i * 3 + 1] = (uy / ul) as f32;
        self.bone_up[i * 3 + 2] = (uz / ul) as f32;
    }

    /// Set a uniform initial velocity (m/s) on every particle.
    /// `ragdoll.js:236-243`. The source defaults `dt = 1 / 120`.
    pub fn set_velocity(&mut self, vx: f64, vy: f64, vz: f64, dt: f64) {
        for i in 0..self.particle_count {
            self.qx[i] = self.px[i] - vx * dt;
            self.qy[i] = self.py[i] - vy * dt;
            self.qz[i] = self.pz[i] - vz * dt;
        }
        self.wake();
    }

    /// Kick the doll at a world point — the killing shot, an explosion, a
    /// melee. `ragdoll.js:250-261`. Falloff is `1/(1+d^2)` so a headshot snaps
    /// the head without teleporting the whole body. The source defaults
    /// `radius = 0.45`, `dt = 1 / 120`.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_impulse(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        ix: f64,
        iy: f64,
        iz: f64,
        radius: f64,
        dt: f64,
    ) {
        for i in 0..self.particle_count {
            let dx = self.px[i] - x;
            let dy = self.py[i] - y;
            let dz = self.pz[i] - z;
            let d2 = dx * dx + dy * dy + dz * dz;
            let w = 1.0 / (1.0 + d2 / (radius * radius));
            let im = self.inv_mass[i];
            self.qx[i] -= ix * im * w * dt;
            self.qy[i] -= iy * im * w * dt;
            self.qz[i] -= iz * im * w * dt;
        }
        self.wake();
    }

    /// `ragdoll.js:263-266`.
    pub fn wake(&mut self) {
        self.sleeping = false;
        self.sleep_timer = 0.0;
    }

    /// One fixed step. `ragdoll.js:268-324`.
    pub fn step(&mut self, dt: f64) {
        if !self.alive || self.sleeping {
            return;
        }
        self.age += dt;
        let n = self.particle_count;
        let g = self.gravity * dt * dt;
        let damp = self.linear_damping;
        let mut motion = 0.0_f64;

        // --- Verlet integration ---
        for i in 0..n {
            if self.inv_mass[i] == 0.0 {
                continue;
            }
            let mut vx = (self.px[i] - self.qx[i]) * damp;
            let mut vy = (self.py[i] - self.qy[i]) * damp;
            let mut vz = (self.pz[i] - self.qz[i]) * damp;
            let vl = hypot3(vx, vy, vz);
            if vl > MAX_PARTICLE_STEP {
                let s = MAX_PARTICLE_STEP / vl;
                vx *= s;
                vy *= s;
                vz *= s;
            }
            self.qx[i] = self.px[i];
            self.qy[i] = self.py[i];
            self.qz[i] = self.pz[i];
            self.px[i] += vx;
            self.py[i] += vy + g;
            self.pz[i] += vz;
            motion += vx * vx + vy * vy + vz * vz;
        }

        // --- Gauss-Seidel constraint solve ---
        for it in 0..self.iterations {
            self.solve_distance();
            self.solve_cones();
            self.solve_contacts(it == self.iterations - 1);
        }
        // One self-collision pass per step: enough to stop an arm sinking
        // through the chest, cheap enough to run on every corpse on screen.
        self.solve_self();

        self.transport_up();
        self.update_aabb();

        // --- sleep ---
        let avg = motion / (1.0_f64).max(n as f64);
        if avg < SLEEP_MOTION * SLEEP_MOTION {
            self.sleep_timer += dt;
            if self.sleep_timer > SLEEP_TIME {
                self.sleeping = true;
                for i in 0..n {
                    self.qx[i] = self.px[i];
                    self.qy[i] = self.py[i];
                    self.qz[i] = self.pz[i];
                }
            }
        } else {
            self.sleep_timer = 0.0;
        }
    }

    /// `ragdoll.js:326-345`.
    fn solve_distance(&mut self) {
        for i in 0..self.bone_count {
            let a = self.bone_head[i] as usize;
            let c = self.bone_tail[i] as usize;
            let wa = self.inv_mass[a];
            let wc = self.inv_mass[c];
            let w = wa + wc;
            if w == 0.0 {
                continue;
            }
            let dx = self.px[c] - self.px[a];
            let dy = self.py[c] - self.py[a];
            let dz = self.pz[c] - self.pz[a];
            let d = hypot3(dx, dy, dz);
            if d < 1e-9 {
                continue;
            }
            // Grouping transcribed literally: `(d - len) / d / w`, two
            // sequential divides, not `(d - len) / (d * w)`.
            let diff = (d - self.bone_len[i] as f64) / d / w;
            self.px[a] += dx * diff * wa;
            self.py[a] += dy * diff * wa;
            self.pz[a] += dz * diff * wa;
            self.px[c] -= dx * diff * wc;
            self.py[c] -= dy * diff * wc;
            self.pz[c] -= dz * diff * wc;
        }
    }

    /// Swing limit: the child bone direction may not deviate from its
    /// parent's by more than `cone`. Correction rotates the child's free end
    /// back onto the cone boundary, weighted by inverse mass so heavy limbs
    /// win. `ragdoll.js:352-420`.
    fn solve_cones(&mut self) {
        for i in 0..self.bone_count {
            let p = self.bone_parent[i];
            if p < 0 {
                continue;
            }
            let p = p as usize;
            let cone = self.bone_cone[i] as f64;
            if cone >= std::f64::consts::PI - 1e-3 {
                continue;
            }

            let pa = self.bone_head[p] as usize;
            let pc = self.bone_tail[p] as usize;
            let mut ax = self.px[pc] - self.px[pa];
            let mut ay = self.py[pc] - self.py[pa];
            let mut az = self.pz[pc] - self.pz[pa];
            let al = hypot3(ax, ay, az);
            if al < 1e-9 {
                continue;
            }
            ax /= al;
            ay /= al;
            az /= al;

            let a = self.bone_head[i] as usize;
            let c = self.bone_tail[i] as usize;
            let mut bx = self.px[c] - self.px[a];
            let mut by = self.py[c] - self.py[a];
            let mut bz = self.pz[c] - self.pz[a];
            let bl = hypot3(bx, by, bz);
            if bl < 1e-9 {
                continue;
            }
            bx /= bl;
            by /= bl;
            bz /= bl;

            let mut dot = ax * bx + ay * by + az * bz;
            if dot > 1.0 {
                dot = 1.0;
            } else if dot < -1.0 {
                dot = -1.0;
            }
            // `angle` gates the correction and is never used in it — the
            // Rodrigues rotation below turns by `cone`, not by `angle`. A
            // last-bit difference in `acos` therefore cannot perturb a
            // trajectory, only flip this comparison at an exact boundary.
            let angle = dot.acos();
            if angle <= cone {
                continue;
            }

            // axis = a x b (fall back to any perpendicular when anti-parallel)
            let mut kx = ay * bz - az * by;
            let mut ky = az * bx - ax * bz;
            let mut kz = ax * by - ay * bx;
            let mut kl = hypot3(kx, ky, kz);
            if kl < 1e-7 {
                kx = -ay;
                ky = ax;
                kz = 0.0;
                kl = hypot3(kx, ky, kz);
                if kl < 1e-7 {
                    kx = 1.0;
                    ky = 0.0;
                    kz = 0.0;
                    kl = 1.0;
                }
            }
            kx /= kl;
            ky /= kl;
            kz /= kl;

            // Rodrigues: rotate the parent direction by `cone` about k -> target dir
            let ca = cone.cos();
            let sa = cone.sin();
            let cross_x = ky * az - kz * ay;
            let cross_y = kz * ax - kx * az;
            let cross_z = kx * ay - ky * ax;
            let kdot = kx * ax + ky * ay + kz * az;
            let tx = ax * ca + cross_x * sa + kx * kdot * (1.0 - ca);
            let ty = ay * ca + cross_y * sa + ky * kdot * (1.0 - ca);
            let tz = az * ca + cross_z * sa + kz * kdot * (1.0 - ca);

            // desired tail position, blended for stability
            let stiff = 0.65;
            let gx = self.px[a] + tx * bl;
            let gy = self.py[a] + ty * bl;
            let gz = self.pz[a] + tz * bl;
            let wa = self.inv_mass[a];
            let wc = self.inv_mass[c];
            let w = wa + wc;
            if w == 0.0 {
                continue;
            }
            let ex = (gx - self.px[c]) * stiff;
            let ey = (gy - self.py[c]) * stiff;
            let ez = (gz - self.pz[c]) * stiff;
            self.px[c] += ex * (wc / w);
            self.py[c] += ey * (wc / w);
            self.pz[c] += ez * (wc / w);
            self.px[a] -= ex * (wa / w);
            self.py[a] -= ey * (wa / w);
            self.pz[a] -= ez * (wa / w);
        }
    }

    /// Capsule bones vs the static world, with friction against the previous
    /// position. `ragdoll.js:423-483`.
    fn solve_contacts(&mut self, apply_friction: bool) {
        // Cloning the `Rc` (a refcount bump) rather than borrowing `self.world`
        // for the body: the loop mutates `self.px`, and Rust will not hand out
        // an immutable borrow of one field alongside a mutable borrow of
        // another through `&mut self` method calls.
        let w = match self.world.clone() {
            Some(w) => w,
            None => return,
        };
        if w.tri_count() == 0 {
            return;
        }
        for i in 0..self.bone_count {
            let a = self.bone_head[i] as usize;
            let c = self.bone_tail[i] as usize;
            let r = self.bone_radius[i] as f64;
            let cts = w.overlap_capsule(
                self.px[a], self.py[a], self.pz[a],
                self.px[c], self.py[c], self.pz[c],
                r,
                self.mask,
                0.0,
            );
            let n = cts.count();
            if n == 0 {
                continue;
            }
            let mut pushx = 0.0_f64;
            let mut pushy = 0.0_f64;
            let mut pushz = 0.0_f64;
            let mut fric = 0.7_f64;
            let mut param = 0.0_f64;
            let mut wsum = 0.0_f64;
            for k in 0..n {
                // `contacts` is a set of `Float32Array`s in the source
                // (`bvh.js:77-90`), and `bvh::Contacts` keeps that width, so
                // every read here is an f32 widened back to f64.
                let d = cts.depth[k] as f64;
                if d <= 1e-5 {
                    continue;
                }
                let nx = cts.nx[k] as f64;
                let ny = cts.ny[k] as f64;
                let nz = cts.nz[k] as f64;
                // Accumulate the *maximum* push along each normal instead of
                // the sum: a tessellated floor would otherwise eject the bone
                // into orbit.
                let already = pushx * nx + pushy * ny + pushz * nz;
                let extra = d - already;
                if extra > 0.0 {
                    pushx += nx * extra;
                    pushy += ny * extra;
                    pushz += nz * extra;
                }
                param += cts.s[k] as f64 * d;
                wsum += d;
                // `SURFACE_PROPS[w.surface[tri]]` — the source guards with
                // `if (sp)`, which can only fail for a surface index outside
                // the twelve-entry table. `Surface` is a closed enum here, so
                // the lookup is total and the guard has nothing to guard.
                let sp = w.surface_of(cts.tri[k] as u32).props();
                fric = sp.friction;
            }
            let pl = hypot3(pushx, pushy, pushz);
            if pl < 1e-6 {
                continue;
            }
            let cap = 0.2;
            if pl > cap {
                let s = cap / pl;
                pushx *= s;
                pushy *= s;
                pushz *= s;
            }
            // Distribute along the capsule so the *contact point* clears the
            // surface rather than the midpoint: classic PBD segment weighting.
            let s_par = if wsum > 0.0 { param / wsum } else { 0.5 };
            let w0 = 1.0 - s_par;
            let w1 = s_par;
            let wa = self.inv_mass[a];
            let wc = self.inv_mass[c];
            let denom = w0 * w0 * wa + w1 * w1 * wc;
            if denom < 1e-12 {
                continue;
            }
            let k0 = (w0 * wa) / denom;
            let k1 = (w1 * wc) / denom;
            self.px[a] += pushx * k0;
            self.py[a] += pushy * k0;
            self.pz[a] += pushz * k0;
            self.px[c] += pushx * k1;
            self.py[c] += pushy * k1;
            self.pz[c] += pushz * k1;

            if apply_friction {
                let mu = (1.0_f64).min(self.friction * fric);
                self.friction_at(a, pushx, pushy, pushz, mu);
                self.friction_at(c, pushx, pushy, pushz, mu);
            }
        }
    }

    /// Capsule-vs-capsule pushout between non-adjacent bones.
    /// `ragdoll.js:486-519`.
    fn solve_self(&mut self) {
        let mut k = 0;
        while k < self.self_pairs.len() {
            let i = self.self_pairs[k] as usize;
            let j = self.self_pairs[k + 1] as usize;
            let a0 = self.bone_head[i] as usize;
            let a1 = self.bone_tail[i] as usize;
            let b0 = self.bone_head[j] as usize;
            let b1 = self.bone_tail[j] as usize;
            let rad = (self.bone_radius[i] as f64 + self.bone_radius[j] as f64) * 0.92;
            let cl = closest_pt_seg_seg(
                self.px[a0], self.py[a0], self.pz[a0],
                self.px[a1], self.py[a1], self.pz[a1],
                self.px[b0], self.py[b0], self.pz[b0],
                self.px[b1], self.py[b1], self.pz[b1],
            );
            let d2 = cl.d2;
            k += 2;
            if d2 >= rad * rad {
                continue;
            }
            let d = d2.sqrt();
            let (nx, ny, nz) = if d > 1e-6 {
                ((cl.ax - cl.bx) / d, (cl.ay - cl.by) / d, (cl.az - cl.bz) / d)
            } else {
                (0.0, 1.0, 0.0)
            };
            let push = (rad - d) * 0.5;
            let s = cl.s;
            let t = cl.t;
            let wa0 = self.inv_mass[a0] * (1.0 - s);
            let wa1 = self.inv_mass[a1] * s;
            let wb0 = self.inv_mass[b0] * (1.0 - t);
            let wb1 = self.inv_mass[b1] * t;
            let wsum = wa0 + wa1 + wb0 + wb1;
            if wsum < 1e-12 {
                continue;
            }
            let k1 = push / wsum;
            self.px[a0] += nx * wa0 * k1;
            self.py[a0] += ny * wa0 * k1;
            self.pz[a0] += nz * wa0 * k1;
            self.px[a1] += nx * wa1 * k1;
            self.py[a1] += ny * wa1 * k1;
            self.pz[a1] += nz * wa1 * k1;
            self.px[b0] -= nx * wb0 * k1;
            self.py[b0] -= ny * wb0 * k1;
            self.pz[b0] -= nz * wb0 * k1;
            self.px[b1] -= nx * wb1 * k1;
            self.py[b1] -= ny * wb1 * k1;
            self.pz[b1] -= nz * wb1 * k1;
        }
    }

    /// `ragdoll.js:521-535`.
    fn friction_at(&mut self, i: usize, nx: f64, ny: f64, nz: f64, mu: f64) {
        let nl = hypot3(nx, ny, nz);
        if nl < 1e-9 {
            return;
        }
        let nx = nx / nl;
        let ny = ny / nl;
        let nz = nz / nl;
        let vx = self.px[i] - self.qx[i];
        let vy = self.py[i] - self.qy[i];
        let vz = self.pz[i] - self.qz[i];
        let vn = vx * nx + vy * ny + vz * nz;
        let tx = vx - nx * vn;
        let ty = vy - ny * vn;
        let tz = vz - nz * vn;
        // Kill the tangential component; PBD friction is applied by moving
        // the previous position towards the current one.
        self.qx[i] += tx * mu;
        self.qy[i] += ty * mu;
        self.qz[i] += tz * mu;
    }

    /// Parallel-transport each bone's reference up-vector so the rendered
    /// roll is continuous, then clamp the twist relative to the parent.
    /// `ragdoll.js:541-586`.
    ///
    /// Note what this does *not* touch: particle positions. `boneUp` feeds
    /// only [`Ragdoll::get_bone_transform`], so the `acos` here can never
    /// perturb the simulation — verified by perturbing `Math.acos` by one ULP
    /// in the capture harness and measuring exactly zero position change over
    /// 600 steps.
    fn transport_up(&mut self) {
        for i in 0..self.bone_count {
            let a = self.bone_head[i] as usize;
            let c = self.bone_tail[i] as usize;
            let mut dx = self.px[c] - self.px[a];
            let mut dy = self.py[c] - self.py[a];
            let mut dz = self.pz[c] - self.pz[a];
            let l = hypot3(dx, dy, dz);
            if l < 1e-9 {
                continue;
            }
            dx /= l;
            dy /= l;
            dz /= l;
            let mut ux = self.bone_up[i * 3] as f64;
            let mut uy = self.bone_up[i * 3 + 1] as f64;
            let mut uz = self.bone_up[i * 3 + 2] as f64;
            let d = ux * dx + uy * dy + uz * dz;
            ux -= dx * d;
            uy -= dy * d;
            uz -= dz * d;
            let ul = hypot3(ux, uy, uz);
            if ul < 1e-5 {
                self.init_up(i);
                continue;
            }
            ux /= ul;
            uy /= ul;
            uz /= ul;

            // twist limit against the parent's frame
            let p = self.bone_parent[i];
            let lim = self.bone_twist[i] as f64;
            if p >= 0 && lim < std::f64::consts::PI - 1e-3 {
                let p = p as usize;
                let mut rx = self.bone_up[p * 3] as f64;
                let mut ry = self.bone_up[p * 3 + 1] as f64;
                let mut rz = self.bone_up[p * 3 + 2] as f64;
                let rd = rx * dx + ry * dy + rz * dz;
                rx -= dx * rd;
                ry -= dy * rd;
                rz -= dz * rd;
                let rl = hypot3(rx, ry, rz);
                if rl > 1e-5 {
                    rx /= rl;
                    ry /= rl;
                    rz /= rl;
                    let mut cs = ux * rx + uy * ry + uz * rz;
                    if cs > 1.0 {
                        cs = 1.0;
                    } else if cs < -1.0 {
                        cs = -1.0;
                    }
                    let ang = cs.acos();
                    if ang > lim {
                        // rotate u back towards r by (ang - lim)
                        let t = (ang - lim) / ang;
                        ux += (rx - ux) * t;
                        uy += (ry - uy) * t;
                        uz += (rz - uz) * t;
                        let nl2_0 = hypot3(ux, uy, uz);
                        let nl2 = if nl2_0 == 0.0 || nl2_0.is_nan() { 1.0 } else { nl2_0 };
                        ux /= nl2;
                        uy /= nl2;
                        uz /= nl2;
                    }
                }
            }
            self.bone_up[i * 3] = ux as f32;
            self.bone_up[i * 3 + 1] = uy as f32;
            self.bone_up[i * 3 + 2] = uz as f32;
        }
    }

    /// `ragdoll.js:588-602`.
    fn update_aabb(&mut self) {
        let mut minx = f64::INFINITY;
        let mut miny = f64::INFINITY;
        let mut minz = f64::INFINITY;
        let mut maxx = f64::NEG_INFINITY;
        let mut maxy = f64::NEG_INFINITY;
        let mut maxz = f64::NEG_INFINITY;
        for i in 0..self.particle_count {
            if self.px[i] < minx {
                minx = self.px[i];
            }
            if self.py[i] < miny {
                miny = self.py[i];
            }
            if self.pz[i] < minz {
                minz = self.pz[i];
            }
            if self.px[i] > maxx {
                maxx = self.px[i];
            }
            if self.py[i] > maxy {
                maxy = self.py[i];
            }
            if self.pz[i] > maxz {
                maxz = self.pz[i];
            }
        }
        self.aabb = Aabb { minx, miny, minz, maxx, maxy, maxz };
    }

    /* ---------------------------------------------------------------- */
    /* Read-back                                                         */
    /* ---------------------------------------------------------------- */

    /// World-space capsule of bone `i`. `ragdoll.js:609-615`.
    pub fn get_bone_capsule(&self, i: usize) -> BoneCapsule {
        let a = self.bone_head[i] as usize;
        let c = self.bone_tail[i] as usize;
        BoneCapsule {
            ax: self.px[a],
            ay: self.py[a],
            az: self.pz[a],
            bx: self.px[c],
            by: self.py[c],
            bz: self.pz[c],
            r: self.bone_radius[i] as f64,
        }
    }

    /// World transform of bone `i`. `ragdoll.js:621-647`.
    ///
    /// The source builds a `THREE.Matrix4` and calls
    /// `Quaternion.setFromRotationMatrix`. Both are inlined here, and the
    /// **storage order matters**: `Matrix4.set` takes its arguments
    /// *row-major* and writes them into a *column-major* `elements` array, so
    /// the source's
    ///
    /// ```text
    /// _m4.set(xx, dx, zx, 0,
    ///         xy, dy, zy, 0,
    ///         xz, dz, zz, 0,
    ///          0,  0,  0, 1)
    /// ```
    ///
    /// yields columns `X = (xx,xy,xz)`, `Y = (dx,dy,dz)`, `Z = (zx,zy,zz)` —
    /// i.e. the basis whose **Y axis runs down the bone**, which is the THREE
    /// bone convention the source's comment describes. Writing this
    /// row-major instead would transpose the rotation and flip every
    /// off-diagonal sign of the quaternion.
    ///
    /// `setFromRotationMatrix` names the elements `m11..m33` in *row, column*
    /// order (`m12 = te[4]`, the second column of the first row), so with the
    /// mapping above: `m11 m12 m13 = xx dx zx`, `m21 m22 m23 = xy dy zy`,
    /// `m31 m32 m33 = xz dz zz`.
    pub fn get_bone_transform(&self, i: usize) -> BoneTransform {
        let a = self.bone_head[i] as usize;
        let c = self.bone_tail[i] as usize;
        let pos = [self.px[a], self.py[a], self.pz[a]];
        let mut dx = self.px[c] - self.px[a];
        let mut dy = self.py[c] - self.py[a];
        let mut dz = self.pz[c] - self.pz[a];
        let l0 = hypot3(dx, dy, dz);
        let l = if l0 == 0.0 || l0.is_nan() { 1.0 } else { l0 };
        dx /= l;
        dy /= l;
        dz /= l;
        let ux = self.bone_up[i * 3] as f64;
        let uy = self.bone_up[i * 3 + 1] as f64;
        let uz = self.bone_up[i * 3 + 2] as f64;
        // basis: Y = bone dir, Z = up-ish, X = Y x Z
        let mut xx = dy * uz - dz * uy;
        let mut xy = dz * ux - dx * uz;
        let mut xz = dx * uy - dy * ux;
        let xl0 = hypot3(xx, xy, xz);
        let xl = if xl0 == 0.0 || xl0.is_nan() { 1.0 } else { xl0 };
        xx /= xl;
        xy /= xl;
        xz /= xl;
        let zx = xy * dz - xz * dy;
        let zy = xz * dx - xx * dz;
        let zz = xx * dy - xy * dx;

        let (m11, m12, m13) = (xx, dx, zx);
        let (m21, m22, m23) = (xy, dy, zy);
        let (m31, m32, m33) = (xz, dz, zz);
        let quat = set_from_rotation_matrix(m11, m12, m13, m21, m22, m23, m31, m32, m33);
        BoneTransform { pos, quat }
    }

    /// `ragdoll.js:697-701`. The `bones3D`/`skeleton` clears have no analogue
    /// here (see the module doc comment's "Not ported").
    pub fn dispose(&mut self) {
        self.alive = false;
    }

    /// Not in the source (JS reads `this.world` directly).
    pub fn world(&self) -> Option<Rc<StaticWorld>> {
        self.world.clone()
    }
}

/// `THREE.Quaternion.setFromRotationMatrix` (three r180,
/// `src/math/Quaternion.js:413-470`), transcribed. Arguments are named in
/// *row, column* order; returns `[x, y, z, w]`.
///
/// Assumes the 3x3 is a pure rotation (unscaled), which the caller guarantees
/// by orthonormalising its basis.
#[allow(clippy::too_many_arguments)]
fn set_from_rotation_matrix(
    m11: f64,
    m12: f64,
    m13: f64,
    m21: f64,
    m22: f64,
    m23: f64,
    m31: f64,
    m32: f64,
    m33: f64,
) -> [f64; 4] {
    let trace = m11 + m22 + m33;
    if trace > 0.0 {
        let s = 0.5 / (trace + 1.0).sqrt();
        [(m32 - m23) * s, (m13 - m31) * s, (m21 - m12) * s, 0.25 / s]
    } else if m11 > m22 && m11 > m33 {
        let s = 2.0 * (1.0 + m11 - m22 - m33).sqrt();
        [0.25 * s, (m12 + m21) / s, (m13 + m31) / s, (m32 - m23) / s]
    } else if m22 > m33 {
        let s = 2.0 * (1.0 + m22 - m11 - m33).sqrt();
        [(m12 + m21) / s, 0.25 * s, (m23 + m32) / s, (m13 - m31) / s]
    } else {
        let s = 2.0 * (1.0 + m33 - m11 - m22).sqrt();
        [(m13 + m31) / s, (m23 + m32) / s, 0.25 * s, (m21 - m12) / s]
    }
}
