//! The physics facade — the world registry, the broadphase, the step order,
//! the query dispatch and the events it emits.
//!
//! Ported from Claude-of-Duty `src/physics/index.js:1-1059` — the whole file.
//!
//! ```text
//! PUBLIC API   const phys = ctx.get('physics')
//!   QUERIES    raycast · raycast_any · line_of_sight · sphere_cast ·
//!              capsule_cast · overlap_capsule · check_capsule ·
//!              overlap_sphere · ground_height
//!   CHARACTER  create_character · remove_character
//!   BALLISTICS fire_bullet · emit_impact · explode
//!   DYNAMICS   add_rigid_body · spawn_debris · remove_rigid_body
//!   RAGDOLLS   create_ragdoll · remove_ragdoll
//!   HITBOXES   add_collider · remove_collider
//!   FRAME      fixed_update · update · late_update
//!   DEBUG      set_debug_draw · toggle_debug_draw · stats
//! ```
//!
//! The nine files under `crates`-equivalent `physics/` are the algorithms;
//! this one is what turns them into a running simulation. Nothing here is a
//! new algorithm — every line either registers something, dispatches a query
//! to one of the ported solvers, or sequences a step.
//!
//! ## The five seams this port had to name
//!
//! 1. **The static world is built once and shared immutably.** Every ported
//!    sibling — [`Character`], [`RigidBodyWorld`], [`Ragdoll`], [`Ballistics`],
//!    [`crate::physics::probe::PhysicsWorld`] — takes an `Rc<StaticWorld>` and
//!    holds it for its whole life. The source's `staticWorld` is instead a
//!    *shared mutable* object: `addStatic` / `removeStatic` / `rebuildStatic`
//!    and the 0.4 s auto-rescan all mutate it in place and every holder sees
//!    the change on the next query. That cannot be expressed against an
//!    `Rc<StaticWorld>`, so [`PhysicsCore::new`] takes an owned
//!    [`StaticWorld`], runs `_ensureStatics`' fallback-ground arm on it,
//!    builds it, and only then publishes the `Rc`. Streaming geometry in later
//!    needs `Rc<RefCell<StaticWorld>>` (or interior mutability inside
//!    `StaticWorld`) across all six of those files — a change that belongs in
//!    `bvh.rs`, not here. See the notes file.
//! 2. **`addStatic` / `addStaticGroup` / `_ensureStatics`' auto-scan are not
//!    ported**, because they flatten a live `THREE.Mesh` and `bvh.js`'s
//!    `addMesh`/`bakeMesh` are themselves not ported (see [`super::bvh`]'s
//!    module doc). What *is* ported is the branch those two paths fall through
//!    to when the scene holds no meshes at all: [`add_fallback_ground`].
//! 3. **The hit and impact ring pools are dropped, deliberately.** `HIT_POOL`
//!    and `IMPACT_POOL` exist in the source to avoid allocating a record per
//!    query, and their cost is the documented hazard at the top of `index.js`:
//!    *"Records come from a 64-deep ring pool: read or copy now, never stash."*
//!    Rust returns [`Hit`] and [`Impact`] by value, which removes the hazard
//!    rather than reproducing it. Nothing observable changes: the source never
//!    reads a pooled record's stale contents, and `_nextHit` resets every field
//!    it does not go on to write. The constants are kept for the record.
//! 4. **`object3D` lives here, not on [`RigidBody`].** `rigidbody.rs` has no
//!    render handle (it is a pure solver), so the facade keeps the body-id →
//!    render-object association and [`PhysicsCore::update`] returns the
//!    interpolated poses as [`InterpolatedPose`] values instead of writing
//!    into a scene graph.
//! 5. **The event vocabulary is forked and physics could not unfork it.**
//!    See "Events" below.
//!
//! ## Events
//!
//! [`crate::events::EventBus`] dispatches on `TypeId`, and three subsystems
//! have each already named their own payload struct for the same event name
//! (`audio::system::ExplosionEvent`, `player::system::ExplosionEvent`,
//! `ui::system::ExplosionEvent`; likewise for `damage:dealt` and
//! `bullet:impact`). A fourth would make the fork worse, so this module
//! defines **no new payload types**:
//!
//! | source                                    | this port                                  |
//! |-------------------------------------------|--------------------------------------------|
//! | consumes `explosion`                      | [`crate::player::system::ExplosionEvent`] — the only fork carrying `damage` |
//! | emits `bullet:impact`                     | [`crate::audio::system::BulletImpact`] — the richest existing fork |
//! | emits `damage:dealt`                      | [`crate::ui::system::DamageDealt`] — the only fork carrying `amount` |
//! | consumes `actor:death`                    | **not wired** — see below |
//!
//! Three fields the source's payloads carry have nowhere to go: `explosion`'s
//! `impulse` (reachable only through the direct [`PhysicsCore::explode`] call
//! here), and `bullet:impact`'s `normal`/`incident`/`surface_index`. Those
//! three *are* ported — they live on the [`Impact`] record
//! [`PhysicsCore::emit_impact`] builds and [`PhysicsCore::fire_bullet`]
//! returns — they simply cannot cross the bus until one canonical payload set
//! exists.
//!
//! `_handleDeath` (`index.js:848-865`) is not wired: its whole body is
//! `createRagdollFromSkeleton`, which needs `specFromSkeleton`/`adoptSkeleton`
//! from `ragdoll.js`, and neither is ported. [`PhysicsCore::ignore_death_events`]
//! is carried so the flag's contract survives until that arm lands.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use axiom_kernel::Seconds;

use crate::config::UNITS;
use crate::engine::Ctx;
use crate::error::CoreError;
use crate::events::SubscriptionId;
use crate::physics::bvh::{Contacts, StaticWorld};
use crate::physics::character::{Character, CharacterOpts};
use crate::physics::debug::{compose, DebugCollider, DebugScene, PhysicsDebugView, RagdollBones};
use crate::physics::math::{closest_pt_seg_seg, ray_capsule, ray_obb, ray_sphere};
use crate::physics::penetration::{Ballistics, Impact as BallisticImpact};
use crate::physics::ragdoll::{Ragdoll, RagdollOpts};
use crate::physics::rigidbody::{RigidBody, RigidBodyWorld, Shape};
use crate::physics::surfaces::{layer, mask, surface_index, SURFACE_PROPS};
use crate::registry::{Phase, Subsystem};
use crate::rng::Rng;
use crate::world::palette::Surface;

/// `index.js:85`. The ring depth the source's `_hitPool` had; see seam 3 in
/// the module doc for why there is no pool here.
pub const HIT_POOL: usize = 64;
/// `index.js:86`. Likewise `_impactPool`.
pub const IMPACT_POOL: usize = 48;

/// The fixed step `fixedUpdate` is driven at, and the `dt` the source's
/// `Ragdoll.setVelocity`/`applyImpulse` default to (`ragdoll.js:236`, `:250`).
const RAGDOLL_DT: f64 = 1.0 / 120.0;

/// `index.js:891` — seconds between auto-rescans while nothing has called
/// `addStatic`.
const AUTO_SCAN_PERIOD: f64 = 0.4;

/// `debug.js:96` — `logRay`'s default time-to-live.
const RAY_TTL: f64 = 1.5;

/// An opaque back-pointer to whatever owns a collider or a ragdoll — the
/// source's `collider.owner` / `ragdoll.actor`, which are `ai` actor objects.
/// There is no actor type at this tier, so the association is carried as a
/// handle the owner minted.
pub type ActorId = u64;

/// An opaque render-object handle — the source's `RigidBody.object3D`.
pub type ObjectId = u64;

/* ================================================================== */
/* THREE.Matrix4 / Quaternion, the parts this file uses               */
/* ================================================================== */

/// `Matrix4.invert()`, transcribed from `three@0.180`'s `math/Matrix4.js`.
///
/// `crate::ai::animator::Mat4` already carries THREE's `compose`, `multiply`
/// and `decompose`, and `physics::debug::compose` carries the composition in
/// this subsystem's own `[f64; 16]` shape — but nothing in the port had needed
/// an inverse before, and both collider raycasting (`index.js:474`) and the
/// rigid-body OBB test (`index.js:528`) do. Transcribed rather than derived:
/// the cofactor expansion's *grouping* is what makes it bit-exact, and any
/// tidier route (a transpose-of-rotation shortcut for the affine case) would
/// silently differ in the last bits.
///
/// A singular matrix yields all zeros, exactly as the source's early return
/// does.
pub fn mat4_invert(te: &[f64; 16]) -> [f64; 16] {
    let (n11, n21, n31, n41) = (te[0], te[1], te[2], te[3]);
    let (n12, n22, n32, n42) = (te[4], te[5], te[6], te[7]);
    let (n13, n23, n33, n43) = (te[8], te[9], te[10], te[11]);
    let (n14, n24, n34, n44) = (te[12], te[13], te[14], te[15]);

    let t11 = n23 * n34 * n42 - n24 * n33 * n42 + n24 * n32 * n43 - n22 * n34 * n43
        - n23 * n32 * n44
        + n22 * n33 * n44;
    let t12 = n14 * n33 * n42 - n13 * n34 * n42 - n14 * n32 * n43 + n12 * n34 * n43
        + n13 * n32 * n44
        - n12 * n33 * n44;
    let t13 = n13 * n24 * n42 - n14 * n23 * n42 + n14 * n22 * n43 - n12 * n24 * n43
        - n13 * n22 * n44
        + n12 * n23 * n44;
    let t14 = n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33
        + n13 * n22 * n34
        - n12 * n23 * n34;

    let det = n11 * t11 + n21 * t12 + n31 * t13 + n41 * t14;
    if det == 0.0 {
        return [0.0; 16];
    }
    let d = 1.0 / det;

    let mut o = [0.0_f64; 16];
    o[0] = t11 * d;
    o[1] = (n24 * n33 * n41 - n23 * n34 * n41 - n24 * n31 * n43 + n21 * n34 * n43
        + n23 * n31 * n44
        - n21 * n33 * n44)
        * d;
    o[2] = (n22 * n34 * n41 - n24 * n32 * n41 + n24 * n31 * n42 - n21 * n34 * n42
        - n22 * n31 * n44
        + n21 * n32 * n44)
        * d;
    o[3] = (n23 * n32 * n41 - n22 * n33 * n41 - n23 * n31 * n42 + n21 * n33 * n42
        + n22 * n31 * n43
        - n21 * n32 * n43)
        * d;

    o[4] = t12 * d;
    o[5] = (n13 * n34 * n41 - n14 * n33 * n41 + n14 * n31 * n43 - n11 * n34 * n43
        - n13 * n31 * n44
        + n11 * n33 * n44)
        * d;
    o[6] = (n14 * n32 * n41 - n12 * n34 * n41 - n14 * n31 * n42 + n11 * n34 * n42
        + n12 * n31 * n44
        - n11 * n32 * n44)
        * d;
    o[7] = (n12 * n33 * n41 - n13 * n32 * n41 + n13 * n31 * n42 - n11 * n33 * n42
        - n12 * n31 * n43
        + n11 * n32 * n43)
        * d;

    o[8] = t13 * d;
    o[9] = (n14 * n23 * n41 - n13 * n24 * n41 - n14 * n21 * n43 + n11 * n24 * n43
        + n13 * n21 * n44
        - n11 * n23 * n44)
        * d;
    o[10] = (n12 * n24 * n41 - n14 * n22 * n41 + n14 * n21 * n42 - n11 * n24 * n42
        - n12 * n21 * n44
        + n11 * n22 * n44)
        * d;
    o[11] = (n13 * n22 * n41 - n12 * n23 * n41 - n13 * n21 * n42 + n11 * n23 * n42
        + n12 * n21 * n43
        - n11 * n22 * n43)
        * d;

    o[12] = t14 * d;
    o[13] = (n13 * n24 * n31 - n14 * n23 * n31 + n14 * n21 * n33 - n11 * n24 * n33
        - n13 * n21 * n34
        + n11 * n23 * n34)
        * d;
    o[14] = (n14 * n22 * n31 - n12 * n24 * n31 - n14 * n21 * n32 + n11 * n24 * n32
        + n12 * n21 * n34
        - n11 * n22 * n34)
        * d;
    o[15] = (n12 * n23 * n31 - n13 * n22 * n31 + n13 * n21 * n32 - n11 * n23 * n32
        - n12 * n21 * n33
        + n11 * n22 * n33)
        * d;
    o
}

/// `Vector3.applyMatrix4(m)`. The perspective divide is kept — THREE always
/// performs it, and `_colliderNormal` feeds this an affine inverse where `w`
/// is 1 but the divide still costs the same bits.
fn apply_matrix4(v: [f64; 3], e: &[f64; 16]) -> [f64; 3] {
    let (x, y, z) = (v[0], v[1], v[2]);
    let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
    [
        (e[0] * x + e[4] * y + e[8] * z + e[12]) * w,
        (e[1] * x + e[5] * y + e[9] * z + e[13]) * w,
        (e[2] * x + e[6] * y + e[10] * z + e[14]) * w,
    ]
}

/// `Vector3.transformDirection(m)` — upper 3x3 then `normalize()`.
/// `Vector3.normalize()` divides by `length() || 1`, and `Vector3.length()`
/// really is the plain root, **not** `Math.hypot`.
fn transform_direction(v: [f64; 3], e: &[f64; 16]) -> [f64; 3] {
    let (x, y, z) = (v[0], v[1], v[2]);
    let nx = e[0] * x + e[4] * y + e[8] * z;
    let ny = e[1] * x + e[5] * y + e[9] * z;
    let nz = e[2] * x + e[6] * y + e[10] * z;
    let l0 = (nx * nx + ny * ny + nz * nz).sqrt();
    // `divideScalar(this.length() || 1)`: JS falsiness catches `0` and `NaN`.
    let l = if l0 == 0.0 || l0.is_nan() { 1.0 } else { l0 };
    [nx / l, ny / l, nz / l]
}

/// `Vector3.lerpVectors(v1, v2, alpha)`.
fn lerp_vectors(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// `Quaternion.copy(a).slerp(b, t)`, transcribed from `three@0.180`'s
/// `math/Quaternion.js`. Component order is `[x, y, z, w]`.
fn slerp(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    if t == 0.0 {
        return a;
    }
    if t == 1.0 {
        return b;
    }
    let (x, y, z, w) = (a[0], a[1], a[2], a[3]);

    let mut cos_half_theta = w * b[3] + x * b[0] + y * b[1] + z * b[2];
    let mut out = if cos_half_theta < 0.0 {
        cos_half_theta = -cos_half_theta;
        [-b[0], -b[1], -b[2], -b[3]]
    } else {
        b
    };

    if cos_half_theta >= 1.0 {
        return [x, y, z, w];
    }

    let sqr_sin_half_theta = 1.0 - cos_half_theta * cos_half_theta;
    if sqr_sin_half_theta <= f64::EPSILON {
        let s = 1.0 - t;
        out = [
            s * x + t * out[0],
            s * y + t * out[1],
            s * z + t * out[2],
            s * w + t * out[3],
        ];
        // `Quaternion.normalize()` — `length()` here is over four components.
        let l0 = (out[0] * out[0] + out[1] * out[1] + out[2] * out[2] + out[3] * out[3]).sqrt();
        if l0 == 0.0 {
            return [0.0, 0.0, 0.0, 1.0];
        }
        return [out[0] / l0, out[1] / l0, out[2] / l0, out[3] / l0];
    }

    let sin_half_theta = sqr_sin_half_theta.sqrt();
    let half_theta = sin_half_theta.atan2(cos_half_theta);
    let ratio_a = ((1.0 - t) * half_theta).sin() / sin_half_theta;
    let ratio_b = (t * half_theta).sin() / sin_half_theta;

    [
        x * ratio_a + out[0] * ratio_b,
        y * ratio_a + out[1] * ratio_b,
        z * ratio_a + out[2] * ratio_b,
        w * ratio_a + out[3] * ratio_b,
    ]
}

/* ================================================================== */
/* Hit records                                                        */
/* ================================================================== */

/// `hit.object` — the opaque thing that was struck.
///
/// The source writes four different kinds of JavaScript object into this one
/// field: `staticWorld.objects[i].mesh` for a triangle hit, `collider.owner`
/// for a collider hit, `body.object3D` for a rigid body and `ragdoll.actor`
/// for a bone. A static batch registered through `addTriangles` has
/// `mesh === null`, and every batch in this port is registered that way
/// (`addMesh` is not ported), so a static hit yields [`HitObject::None`] here
/// exactly as it does there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitObject {
    #[default]
    None,
    /// A collider's `owner`, or a ragdoll's `actor`.
    Actor(ActorId),
    /// A rigid body's `object3D`.
    Render(ObjectId),
}

/// `makePublicHit()`. `index.js:88-106`.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub hit: bool,
    pub point: [f64; 3],
    pub normal: [f64; 3],
    pub distance: f64,
    pub fraction: f64,
    pub surface: Surface,
    pub object: HitObject,
    /// [`Collider::id`].
    pub collider: Option<u32>,
    /// [`RigidBody::id`].
    pub body: Option<i32>,
    /// [`Ragdoll::id`].
    pub ragdoll: Option<i32>,
    pub actor: Option<ActorId>,
    /// A collider's `part`, or the struck bone's spec name.
    pub part: Option<String>,
    pub triangle: i32,
    pub front_face: bool,
}

impl Default for Hit {
    /// `makePublicHit()` and `_nextHit()` agree on every field but `point` and
    /// `normal`, which `_nextHit` leaves as the previous query left them and
    /// every path that reads them writes first.
    fn default() -> Self {
        Hit {
            hit: false,
            point: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            distance: f64::INFINITY,
            fraction: 1.0,
            surface: Surface::Concrete,
            object: HitObject::None,
            collider: None,
            body: None,
            ragdoll: None,
            actor: None,
            part: None,
            triangle: -1,
            front_face: true,
        }
    }
}

/// One entry of the source's `_impactPool` (`index.js:205-217`) — the payload
/// `emitImpact` fills and dispatches as `bullet:impact`.
#[derive(Debug, Clone, PartialEq)]
pub struct Impact {
    pub point: [f64; 3],
    pub normal: [f64; 3],
    pub incident: [f64; 3],
    pub surface: Surface,
    pub damage: f64,
    pub exit: bool,
    pub object: HitObject,
    pub body: Option<i32>,
    pub actor: Option<ActorId>,
    pub part: Option<String>,
}

impl Default for Impact {
    fn default() -> Self {
        Impact {
            point: [0.0; 3],
            normal: [0.0; 3],
            incident: [0.0; 3],
            surface: Surface::Concrete,
            damage: 0.0,
            exit: false,
            object: HitObject::None,
            body: None,
            actor: None,
            part: None,
        }
    }
}

/* ================================================================== */
/* Collider                                                           */
/* ================================================================== */

/// `collider.shape`. The source's `'capsule' | 'sphere' | 'box'` strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColliderShape {
    #[default]
    Capsule,
    Sphere,
    Box,
}

/// `addCollider(opts)`'s bag. `index.js:112-133`.
#[derive(Debug, Clone)]
pub struct ColliderOpts {
    /// `opts.shape ?? 'capsule'`.
    pub shape: ColliderShape,
    /// `opts.radius ?? 0.2`.
    pub radius: f64,
    /// `opts.hx/hy/hz ?? 0.2`.
    pub half_extents: [f64; 3],
    /// `opts.layer ?? LAYER.ACTOR`.
    pub layer: u16,
    /// `surfaceIndex(opts.surface ?? 'flesh')`.
    pub surface: Surface,
    pub owner: Option<ActorId>,
    pub part: Option<String>,
    /// `opts.damageScale ?? 1`.
    pub damage_scale: f64,
    /// `opts.enabled !== false`.
    pub enabled: bool,
    /// `opts.p0`/`opts.p1` — when both are present the constructor calls
    /// `setSegment` with them.
    pub segment: Option<([f64; 3], [f64; 3])>,
    /// `opts.center` — when present the constructor calls `setSphere`.
    pub center: Option<[f64; 3]>,
}

impl Default for ColliderOpts {
    fn default() -> Self {
        ColliderOpts {
            shape: ColliderShape::Capsule,
            radius: 0.2,
            half_extents: [0.2, 0.2, 0.2],
            layer: layer::ACTOR,
            surface: Surface::Flesh,
            owner: None,
            part: None,
            damage_scale: 1.0,
            enabled: true,
            segment: None,
            center: None,
        }
    }
}

/// A moving convex proxy — AI hitboxes, doors, dropped weapons, elevators.
/// `index.js:111-166` (`class Collider`).
#[derive(Debug, Clone, PartialEq)]
pub struct Collider {
    pub id: u32,
    pub shape: ColliderShape,
    pub ax: f64,
    pub ay: f64,
    pub az: f64,
    pub bx: f64,
    pub by: f64,
    pub bz: f64,
    pub radius: f64,
    pub hx: f64,
    pub hy: f64,
    pub hz: f64,
    /// THREE's `elements`, column-major.
    pub matrix: [f64; 16],
    pub inverse: [f64; 16],
    pub layer: u16,
    pub surface: Surface,
    pub owner: Option<ActorId>,
    pub part: Option<String>,
    pub damage_scale: f64,
    pub enabled: bool,
}

const IDENTITY4: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

impl Collider {
    fn new(id: u32, opts: ColliderOpts) -> Self {
        let mut c = Collider {
            id,
            shape: opts.shape,
            ax: 0.0,
            ay: 0.0,
            az: 0.0,
            bx: 0.0,
            by: 0.0,
            bz: 0.0,
            radius: opts.radius,
            hx: opts.half_extents[0],
            hy: opts.half_extents[1],
            hz: opts.half_extents[2],
            matrix: IDENTITY4,
            inverse: IDENTITY4,
            layer: opts.layer,
            surface: opts.surface,
            owner: opts.owner,
            part: opts.part,
            damage_scale: opts.damage_scale,
            enabled: opts.enabled,
        };
        if let Some((p0, p1)) = opts.segment {
            c.set_segment(p0[0], p0[1], p0[2], p1[0], p1[1], p1[2], None);
        }
        if let Some(p) = opts.center {
            let r = c.radius;
            c.set_sphere(p[0], p[1], p[2], Some(r));
        }
        c
    }

    /// `setSegment(ax, ay, az, bx, by, bz, r)`. `index.js:135-140`.
    #[allow(clippy::too_many_arguments)]
    pub fn set_segment(
        &mut self,
        ax: f64,
        ay: f64,
        az: f64,
        bx: f64,
        by: f64,
        bz: f64,
        r: Option<f64>,
    ) -> &mut Self {
        self.ax = ax;
        self.ay = ay;
        self.az = az;
        self.bx = bx;
        self.by = by;
        self.bz = bz;
        if let Some(r) = r {
            self.radius = r;
        }
        self
    }

    /// `setSphere(x, y, z, r)`. `index.js:142-149`. Note it also *changes the
    /// shape*, which is why a capsule collider silently becomes a sphere the
    /// first time an owner positions it this way.
    pub fn set_sphere(&mut self, x: f64, y: f64, z: f64, r: Option<f64>) -> &mut Self {
        self.ax = x;
        self.bx = x;
        self.ay = y;
        self.by = y;
        self.az = z;
        self.bz = z;
        if let Some(r) = r {
            self.radius = r;
        }
        self.shape = ColliderShape::Sphere;
        self
    }

    /// `setFromObject(obj, hx, hy, hz)`. `index.js:152-159`. The source reads
    /// `obj.matrixWorld` after forcing an update; there is no scene graph
    /// here, so the caller passes the world matrix it would have read.
    pub fn set_from_matrix(&mut self, m: [f64; 16], half_extents: Option<[f64; 3]>) -> &mut Self {
        self.matrix = m;
        self.inverse = mat4_invert(&m);
        if let Some(h) = half_extents {
            self.hx = h[0];
            self.hy = h[1];
            self.hz = h[2];
        }
        self.shape = ColliderShape::Box;
        self
    }

    /// `setMatrix(m)`. `index.js:161-165`. Unlike [`Collider::set_from_matrix`]
    /// this does **not** change the shape.
    pub fn set_matrix(&mut self, m: [f64; 16]) -> &mut Self {
        self.matrix = m;
        self.inverse = mat4_invert(&m);
        self
    }
}

impl DebugCollider for Collider {
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn is_box(&self) -> bool {
        self.shape == ColliderShape::Box
    }
    fn matrix(&self) -> [f64; 16] {
        self.matrix
    }
    fn half_extents(&self) -> [f64; 3] {
        [self.hx, self.hy, self.hz]
    }
    fn segment(&self) -> [f64; 6] {
        [self.ax, self.ay, self.az, self.bx, self.by, self.bz]
    }
    fn radius(&self) -> f64 {
        self.radius
    }
}

/// [`Ragdoll`] read through the debug view's four-method window. The trait is
/// declared in `physics/debug.rs` and points at this file for the type that
/// would implement it; this is that implementation.
struct RagdollView<'a>(&'a Ragdoll);

impl RagdollBones for RagdollView<'_> {
    fn bone_count(&self) -> usize {
        self.0.bone_count
    }
    fn bone_head(&self, i: usize) -> usize {
        self.0.bone_head[i] as usize
    }
    fn bone_tail(&self, i: usize) -> usize {
        self.0.bone_tail[i] as usize
    }
    fn bone_radius(&self, i: usize) -> f64 {
        f64::from(self.0.bone_radius[i])
    }
    fn particle(&self, p: usize) -> [f64; 3] {
        [self.0.px[p], self.0.py[p], self.0.pz[p]]
    }
}

/* ================================================================== */
/* Option bags                                                        */
/* ================================================================== */

/// `addRigidBody(opts)`. Mirrors the `RigidBody` constructor's option bag
/// (`rigidbody.js:27-94`); `None` selects the source's `??` default.
#[derive(Debug, Clone, Default)]
pub struct RigidBodyOpts {
    pub shape: Option<Shape>,
    /// `opts.halfExtents`, defaulting to `0.1` per axis.
    pub half_extents: Option<[f64; 3]>,
    /// `opts.radius ?? Math.min(hx, hy, hz)`.
    pub radius: Option<f64>,
    /// `opts.halfHeight ?? Math.max(0, hy - radius)`.
    pub half_height: Option<f64>,
    pub mass: Option<f64>,
    pub position: Option<[f64; 3]>,
    pub quaternion: Option<[f64; 4]>,
    pub velocity: Option<[f64; 3]>,
    pub angular_velocity: Option<[f64; 3]>,
    pub restitution: Option<f64>,
    pub friction: Option<f64>,
    pub linear_damping: Option<f64>,
    pub angular_damping: Option<f64>,
    pub gravity_scale: Option<f64>,
    pub surface: Option<Surface>,
    /// `opts.surfaceType` — a *name*, applied after construction
    /// (`index.js:774`), so it wins over `opts.surface`.
    pub surface_type: Option<String>,
    pub mask: Option<u16>,
    pub layer: Option<u16>,
    pub ccd: Option<bool>,
    pub lifetime: Option<f64>,
    /// `opts.object3D`; kept by the facade, not by the solver.
    pub object3d: Option<ObjectId>,
}

/// `spawnDebris(position, velocity, opts)`. `index.js:784-804`.
#[derive(Debug, Clone, Default)]
pub struct DebrisOpts {
    /// `opts.size ?? 0.08`.
    pub size: Option<f64>,
    /// `opts.shape ?? 'box'`.
    pub shape: Option<Shape>,
    /// `surfaceIndex(opts.surface ?? 'concrete')`.
    pub surface: Option<String>,
    pub mass: Option<f64>,
    pub restitution: Option<f64>,
    pub friction: Option<f64>,
    /// `opts.lifetime ?? 20`.
    pub lifetime: Option<f64>,
    pub object3d: Option<ObjectId>,
}

/// `explode(e)`. `index.js:747-766`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Explosion {
    pub position: [f64; 3],
    /// `e.radius ?? 5`.
    pub radius: Option<f64>,
    /// `e.impulse ?? (e.damage ?? 100) * 0.9`.
    pub impulse: Option<f64>,
    pub damage: Option<f64>,
}

/// `fireBullet(opts)`. `index.js:708-714`, forwarded to `Ballistics.fire`.
#[derive(Debug, Clone, Copy)]
pub struct BulletOpts {
    pub origin: [f64; 3],
    pub dir: [f64; 3],
    /// `penetration.js:60-70` defaults: 400 m, 34 damage, power 1.0,
    /// `MASK.BULLET`, dropoff 0.55.
    pub max_dist: f64,
    pub damage: f64,
    pub penetration: f64,
    pub mask: u16,
    pub dropoff: f64,
}

impl Default for BulletOpts {
    fn default() -> Self {
        BulletOpts {
            origin: [0.0; 3],
            dir: [0.0, 0.0, -1.0],
            max_dist: 400.0,
            damage: 34.0,
            penetration: 1.0,
            mask: mask::BULLET,
            dropoff: 0.55,
        }
    }
}

/// `createRagdoll(opts)`'s extra arms on top of [`RagdollOpts`] —
/// `index.js:815-822`.
#[derive(Debug, Clone, Default)]
pub struct RagdollSpawn {
    pub opts: RagdollOpts,
    /// `opts.velocity` → `rd.setVelocity(...)`.
    pub velocity: Option<[f64; 3]>,
    /// `opts.impulse` + `opts.point` → `rd.applyImpulse(...)`; both required.
    pub impulse: Option<[f64; 3]>,
    pub point: Option<[f64; 3]>,
    /// `opts.impulseRadius ?? 0.45`.
    pub impulse_radius: Option<f64>,
    /// `rd.actor` — set by `createRagdollFromSkeleton`, not by `createRagdoll`.
    pub actor: Option<ActorId>,
}

/// One rigid body's interpolated render pose. `update()`'s output
/// (`index.js:915-932`); see seam 4 in the module doc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InterpolatedPose {
    pub object: ObjectId,
    pub body: i32,
    pub position: [f64; 3],
    pub quaternion: [f64; 4],
}

/// `this.stats`. `index.js:234-238`, minus `buildMs`/`stepMs`, which are
/// `performance.now()` deltas — wall clock has no place in a deterministic
/// port and nothing reads them but the dev overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhysicsStats {
    pub triangles: usize,
    pub nodes: usize,
    pub objects: usize,
    pub bodies: usize,
    pub awake: usize,
    pub ragdolls: usize,
    pub characters: usize,
    pub colliders: usize,
    pub raycasts: u64,
}

/// A ragdoll plus the `actor` back-pointer `ragdoll.rs` deliberately dropped.
struct RagdollEntry {
    doll: Ragdoll,
    actor: Option<ActorId>,
}

/* ================================================================== */
/* _addFallbackGround                                                 */
/* ================================================================== */

/// `_addFallbackGround()`. `index.js:378-385`.
///
/// **`Float32Array`.** The source builds the soup in one
/// (`index.js:380`), so every coordinate is stored rounded to `f32` before the
/// BVH ever sees it. Here `S = 300` and the other coordinate is `0`, both of
/// which `f32` holds exactly, so the width happens not to bite — but the cast
/// is written out rather than assumed, because "it happens not to bite" is a
/// fact about *these* two constants and not about the code. The golden asserts
/// the round trip.
pub fn add_fallback_ground(world: &mut StaticWorld) -> i32 {
    const S: f64 = 300.0;
    let tris: [f64; 18] = [
        -S, 0.0, -S, -S, 0.0, S, S, 0.0, S, //
        -S, 0.0, -S, S, 0.0, S, S, 0.0, -S,
    ];
    let stored: Vec<f64> = tris.iter().map(|v| *v as f32 as f64).collect();
    world.add_triangles(
        &stored,
        2,
        Surface::Concrete,
        layer::STATIC,
        "physics:fallback-ground",
    )
}

/// The pre-build static registry — the only window in which geometry can be
/// registered, because after [`PhysicsCore::new`] the world is behind an `Rc`
/// every solver shares (seam 1).
///
/// It is also where the live-batch count lives: `stats.objects` is the
/// source's `for (const o of staticWorld.objects) if (o && o.alive) n++`
/// (`index.js:950-951`), and [`StaticWorld`] exposes no object accessor to
/// count over. Adding `StaticWorld::object_count()` is the right fix and it
/// belongs in `bvh.rs`, which is another slice's file — see the notes.
///
/// It also carries `explicit`, the source's `_explicitStatics`
/// (`index.js:292`). Every [`StaticRegistry::add_triangles`] is one `addStatic`
/// call, and `addStatic` is what makes `_ensureStatics` return at its first
/// line (`index.js:330`) — which is what suppresses both the auto-rescan and
/// the last-resort ground plane. See [`PhysicsCore::new`].
pub struct StaticRegistry {
    world: StaticWorld,
    live: usize,
    explicit: usize,
}

impl Default for StaticRegistry {
    fn default() -> Self {
        StaticRegistry::new()
    }
}

impl StaticRegistry {
    pub fn new() -> Self {
        StaticRegistry {
            world: StaticWorld::new(),
            live: 0,
            explicit: 0,
        }
    }

    /// A world someone else already populated. `live` is how many batches it
    /// holds; pass `0` if it holds none.
    ///
    /// A non-zero `live` is an explicit registration by that someone: it is the
    /// same fact `addStatic` records, arriving pre-baked instead of one call at
    /// a time. A world handed over with `live == 0` holds nothing, so the
    /// fallback ground is still the right last resort for it.
    pub fn from_world(world: StaticWorld, live: usize) -> Self {
        StaticRegistry {
            world,
            live,
            explicit: live,
        }
    }

    /// `staticWorld.addTriangles(...)` — the registration `addStatic` performs
    /// once `bakeMesh` has flattened a mesh.
    pub fn add_triangles(
        &mut self,
        positions: &[f64],
        count: usize,
        surface: Surface,
        mask: u16,
        name: &str,
    ) -> i32 {
        self.live += 1;
        self.explicit += 1;
        self.world.add_triangles(positions, count, surface, mask, name)
    }

    /// `removeStatic(handle)`. `index.js:310-314`.
    ///
    /// Does **not** decrement `explicit`, because `removeStatic` does not
    /// decrement `_explicitStatics`: once a world has registered its own
    /// geometry it never falls back to the auto-scan, even if every batch is
    /// later removed.
    pub fn remove_object(&mut self, id: i32) -> bool {
        let removed = self.world.remove_object(id);
        if removed {
            self.live -= 1;
        }
        removed
    }
}

/// `segmentHitsAabb(...)`. `index.js:1044-1057` — the ragdoll broadphase gate.
/// `ab` is `[minx, miny, minz, maxx, maxy, maxz]`.
#[allow(clippy::too_many_arguments)]
pub fn segment_hits_aabb(
    ox: f64,
    oy: f64,
    oz: f64,
    dx: f64,
    dy: f64,
    dz: f64,
    len: f64,
    ab: [f64; 6],
    pad: f64,
) -> bool {
    let ix = 1.0 / if dx != 0.0 { dx } else { 1e-30 };
    let iy = 1.0 / if dy != 0.0 { dy } else { 1e-30 };
    let iz = 1.0 / if dz != 0.0 { dz } else { 1e-30 };
    let mut t0 = (ab[0] - pad - ox) * ix;
    let mut t1 = (ab[3] + pad - ox) * ix;
    let mut lo = t0.min(t1);
    let mut hi = t0.max(t1);
    t0 = (ab[1] - pad - oy) * iy;
    t1 = (ab[4] + pad - oy) * iy;
    lo = lo.max(t0.min(t1));
    hi = hi.min(t0.max(t1));
    t0 = (ab[2] - pad - oz) * iz;
    t1 = (ab[5] + pad - oz) * iz;
    lo = lo.max(t0.min(t1));
    hi = hi.min(t0.max(t1));
    hi >= 0.0_f64.max(lo) && lo <= len
}

/* ================================================================== */
/* PhysicsCore                                                        */
/* ================================================================== */

/// `class PhysicsSystem`. `index.js:176-1040`.
///
/// The frame loop and every event handler reach the same mutable state, which
/// is what JavaScript's `this` is; here that is one `RefCell`-wrapped core and
/// a thin [`PhysicsSystem`] around it, the same shape
/// [`crate::audio::system::AudioSystem`] uses and for the same reason.
pub struct PhysicsCore {
    static_world: Rc<StaticWorld>,
    pub bodies: RigidBodyWorld,
    characters: Vec<Character>,
    ragdolls: Vec<RagdollEntry>,
    colliders: Vec<Collider>,
    ballistics: Ballistics,

    /// Body id → the render object the app associated with it. The source's
    /// `RigidBody.object3D`; see seam 4.
    render_objects: HashMap<i32, ObjectId>,

    rng: Rc<RefCell<Rng>>,
    next_collider_id: u32,

    pub gravity: f64,
    /// `index.js:196` — set true by `ai` if it wants to own ragdoll creation.
    pub ignore_death_events: bool,
    /// `index.js:197`.
    pub max_ragdolls: usize,

    fallback_id: i32,
    /// See [`StaticRegistry`]: the count `stats.objects` reports.
    live_objects: usize,
    explicit_statics: usize,
    auto_scan_timer: f64,

    pub debug: Option<PhysicsDebugView>,
    pub stats: PhysicsStats,
    ray_count: u64,

    /// The bus, captured at `init` so `emit_impact` can dispatch. `None`
    /// before init — every emit is then a no-op, matching the source's
    /// `this.ctx` being undefined until `init` runs.
    events: Option<crate::events::EventBus>,
}

impl PhysicsCore {
    /// `new PhysicsSystem()` plus the parts of `init` that do not need a
    /// `Ctx` (`index.js:180-240`, `:256`).
    ///
    /// `world` is the caller's triangle soup, **not yet built**. It is built
    /// here and published as the shared `Rc<StaticWorld>` every solver holds.
    /// See seam 1.
    ///
    /// **The fallback ground is a last resort, not a default.** `_ensureStatics`
    /// returns at its first line when `_explicitStatics > 0` (`index.js:330`),
    /// and even when it runs it only adds the plane if nothing else registered
    /// geometry (`if (this._autoIds.length === 0 …)`, `index.js:364`). This
    /// used to add it unconditionally, which is harmless for an empty registry
    /// and wrong for every populated one: a 600 x 600 m concrete plane at
    /// y = 0, under the real level, catching bullets, debris and ragdolls
    /// through the floor and answering `ground_height` where the level has no
    /// floor at all. A registry that registered anything gets no plane.
    pub fn new(registry: StaticRegistry) -> Self {
        let StaticRegistry {
            mut world,
            live: live_objects,
            explicit,
        } = registry;
        let fallback_id = (explicit == 0)
            .then(|| add_fallback_ground(&mut world))
            .unwrap_or(-1);
        world.build();
        // The fallback ground, when it fired, is one more live batch.
        let live = live_objects + usize::from(fallback_id >= 0);
        PhysicsCore::over(Rc::new(world), live, explicit, fallback_id)
    }

    /// A world somebody else registered **and already built** — the shared
    /// `Rc<StaticWorld>` `crate::scene::level` hands out, so the game runs one
    /// BVH rather than two.
    ///
    /// This is the source's `addStatic` case exactly: `_explicitStatics > 0`,
    /// so `_ensureStatics` returns immediately (no auto-scan, no fallback
    /// ground), and `staticWorld.dirty` is false, so `fixed_update` never
    /// rebuilds. Seam 1's immutability contract is unchanged — the world was
    /// already frozen behind an `Rc` before it got here.
    ///
    /// `live_objects` is what `stats.objects` reports: how many batches the
    /// builder registered. [`StaticWorld`] exposes no object count (see
    /// [`StaticRegistry`]), so the builder is the only one who knows.
    pub fn with_static_world(world: Rc<StaticWorld>, live_objects: usize) -> Self {
        // Only `> 0` is ever observed (`index.js:330`); the exact count of
        // `addStatic` calls the builder made is not recoverable from an
        // already-built world, and nothing reads it.
        PhysicsCore::over(world, live_objects, 1, -1)
    }

    /// The shared body of both constructors: everything after the static world
    /// is settled.
    fn over(
        world: Rc<StaticWorld>,
        live_objects: usize,
        explicit_statics: usize,
        fallback_id: i32,
    ) -> Self {
        let rng = Rc::new(RefCell::new(Rng::new(0)));
        let mut ballistics = Ballistics::new();
        ballistics.set_rng(Rc::clone(&rng));

        let mut core = PhysicsCore {
            bodies: RigidBodyWorld::new(Rc::clone(&world), UNITS.gravity),
            static_world: world,
            characters: Vec::new(),
            ragdolls: Vec::new(),
            colliders: Vec::new(),
            ballistics,
            render_objects: HashMap::new(),
            rng,
            next_collider_id: 1,
            gravity: UNITS.gravity,
            ignore_death_events: false,
            max_ragdolls: 8,
            fallback_id,
            live_objects,
            explicit_statics,
            auto_scan_timer: 0.0,
            debug: Some(PhysicsDebugView::new()),
            stats: PhysicsStats::default(),
            ray_count: 0,
            events: None,
        };
        core.sync_stats();
        core
    }

    /// The shared world handle — for a caller that needs the BVH directly
    /// (the character controller, [`crate::physics::probe::PhysicsWorld`]).
    pub fn static_world(&self) -> Rc<StaticWorld> {
        Rc::clone(&self.static_world)
    }

    /// `index.js:321-323`.
    pub fn triangle_count(&self) -> usize {
        self.static_world.tri_count()
    }

    /// `this._fallbackId`.
    pub fn fallback_id(&self) -> i32 {
        self.fallback_id
    }

    /// The physics stream. `init` sets it to `ctx.rng.fork()`
    /// (`index.js:244-245`); before that it is a fixed `Rng::new(0)` so the
    /// core is usable in isolation.
    pub fn set_rng(&mut self, rng: Rng) {
        *self.rng.borrow_mut() = rng;
    }

    pub fn rng(&self) -> Rc<RefCell<Rng>> {
        Rc::clone(&self.rng)
    }

    /// The bus `emit_impact` and `explode` dispatch on — `this.ctx = ctx`
    /// (`index.js:243`), the half of it this core actually reads.
    ///
    /// The counterpart of [`PhysicsCore::set_rng`], and it was missing: the
    /// field could only be written from inside [`Subsystem::init`], so a caller
    /// that drives the core without a [`Ctx`] (there is no way to build one
    /// outside `crate::engine`) got a core that silently emitted nothing.
    /// [`PlayerCore::init`](crate::player::system::PlayerCore::init) already
    /// takes its bus as an argument; this is physics saying the same thing.
    pub fn set_events(&mut self, events: crate::events::EventBus) {
        self.events = Some(events);
    }

    /* ---------------------------------------------------------------- */
    /* Queries                                                           */
    /* ---------------------------------------------------------------- */

    /// Closest-hit ray. `index.js:415-467`. `dir` need not be normalised.
    /// Always returns a record — test `.hit`.
    pub fn raycast(
        &mut self,
        origin: [f64; 3],
        dir: [f64; 3],
        max_dist: f64,
        mask: u16,
    ) -> Hit {
        let (ox, oy, oz) = (origin[0], origin[1], origin[2]);
        let (mut dx, mut dy, mut dz) = (dir[0], dir[1], dir[2]);
        let mut out = Hit::default();

        // `Math.hypot`, not the plain root: `index.js:427`.
        let l = crate::jsmath::hypot3(dx, dy, dz);
        if l < 1e-9 {
            out.point = [ox, oy, oz];
            out.distance = 0.0;
            return out;
        }
        dx /= l;
        dy /= l;
        dz /= l;
        self.ray_count += 1;
        let mut best = max_dist;

        let raw = self
            .static_world
            .raycast(ox, oy, oz, dx, dy, dz, best, mask, -1);
        if raw.hit {
            best = raw.t;
            out.hit = true;
            out.distance = raw.t;
            out.point = [raw.px, raw.py, raw.pz];
            out.normal = [raw.nx, raw.ny, raw.nz];
            out.surface = Surface::ALL[raw.surface as usize];
            out.triangle = raw.tri;
            out.front_face = raw.front_face;
            // `staticWorld.objects[raw.object]?.mesh ?? null` — always null
            // here; see `HitObject`.
            out.object = HitObject::None;
        }

        best = self.raycast_colliders(ox, oy, oz, dx, dy, dz, best, mask, &mut out);
        best = self.raycast_bodies(ox, oy, oz, dx, dy, dz, best, mask, &mut out);
        self.raycast_ragdolls(ox, oy, oz, dx, dy, dz, best, mask, &mut out);

        if out.hit {
            out.fraction = out.distance / max_dist;
        } else {
            out.point = [ox + dx * max_dist, oy + dy * max_dist, oz + dz * max_dist];
            out.normal = [-dx, -dy, -dz];
            out.distance = max_dist;
            out.surface = Surface::Concrete;
        }
        if let Some(d) = self.debug.as_mut() {
            if d.enabled {
                d.log_ray(ox, oy, oz, out.point[0], out.point[1], out.point[2], RAY_TTL);
            }
        }
        out
    }

    /// `_raycastColliders`. `index.js:469-493`.
    #[allow(clippy::too_many_arguments)]
    fn raycast_colliders(
        &self,
        ox: f64,
        oy: f64,
        oz: f64,
        dx: f64,
        dy: f64,
        dz: f64,
        mut best: f64,
        mask: u16,
        out: &mut Hit,
    ) -> f64 {
        for c in &self.colliders {
            if !c.enabled || (c.layer & mask) == 0 {
                continue;
            }
            let t = if c.shape == ColliderShape::Box {
                ray_obb(ox, oy, oz, dx, dy, dz, &c.inverse, c.hx, c.hy, c.hz, best)
            } else {
                ray_capsule(
                    ox, oy, oz, dx, dy, dz, c.ax, c.ay, c.az, c.bx, c.by, c.bz, c.radius, best,
                )
            };
            if t < 0.0 || t >= best {
                continue;
            }
            best = t;
            out.hit = true;
            out.distance = t;
            out.point = [ox + dx * t, oy + dy * t, oz + dz * t];
            out.normal = collider_normal(c, out.point, dx, dy, dz);
            out.surface = c.surface;
            out.object = c.owner.map_or(HitObject::None, HitObject::Actor);
            out.collider = Some(c.id);
            out.actor = c.owner;
            out.part = c.part.clone();
            out.body = None;
            out.ragdoll = None;
            out.triangle = -1;
            out.front_face = true;
        }
        best
    }

    /// `_raycastBodies`. `index.js:517-552`.
    #[allow(clippy::too_many_arguments)]
    fn raycast_bodies(
        &self,
        ox: f64,
        oy: f64,
        oz: f64,
        dx: f64,
        dy: f64,
        dz: f64,
        mut best: f64,
        mask: u16,
        out: &mut Hit,
    ) -> f64 {
        if (mask & layer::DEBRIS) == 0 {
            return best;
        }
        for b in self.bodies.bodies() {
            let t = if b.shape == Shape::Sphere {
                ray_sphere(
                    ox,
                    oy,
                    oz,
                    dx,
                    dy,
                    dz,
                    b.position[0],
                    b.position[1],
                    b.position[2],
                    b.radius,
                    best,
                )
            } else {
                let m = compose(b.position, b.quaternion, [1.0, 1.0, 1.0]);
                let mi = mat4_invert(&m);
                if b.shape == Shape::Capsule {
                    ray_obb(
                        ox,
                        oy,
                        oz,
                        dx,
                        dy,
                        dz,
                        &mi,
                        b.radius,
                        b.half_height + b.radius,
                        b.radius,
                        best,
                    )
                } else {
                    ray_obb(ox, oy, oz, dx, dy, dz, &mi, b.hx, b.hy, b.hz, best)
                }
            };
            if t < 0.0 || t >= best {
                continue;
            }
            best = t;
            out.hit = true;
            out.distance = t;
            out.point = [ox + dx * t, oy + dy * t, oz + dz * t];
            let mut n = [
                out.point[0] - b.position[0],
                out.point[1] - b.position[1],
                out.point[2] - b.position[2],
            ];
            // `Vector3.lengthSq()`, then `normalize()` — a plain root.
            let len_sq = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
            if len_sq < 1e-12 {
                n = [-dx, -dy, -dz];
            } else {
                let l0 = len_sq.sqrt();
                let l = if l0 == 0.0 || l0.is_nan() { 1.0 } else { l0 };
                n = [n[0] / l, n[1] / l, n[2] / l];
            }
            out.normal = n;
            out.surface = Surface::ALL[b.surface as usize];
            out.object = self
                .render_objects
                .get(&b.id)
                .map_or(HitObject::None, |o| HitObject::Render(*o));
            out.body = Some(b.id);
            out.collider = None;
            out.ragdoll = None;
            out.triangle = -1;
        }
        best
    }

    /// `_raycastRagdolls`. `index.js:554-594`.
    #[allow(clippy::too_many_arguments)]
    fn raycast_ragdolls(
        &self,
        ox: f64,
        oy: f64,
        oz: f64,
        dx: f64,
        dy: f64,
        dz: f64,
        mut best: f64,
        mask: u16,
        out: &mut Hit,
    ) -> f64 {
        if (mask & layer::RAGDOLL) == 0 {
            return best;
        }
        for entry in &self.ragdolls {
            let rd = &entry.doll;
            let ab = [
                rd.aabb.minx,
                rd.aabb.miny,
                rd.aabb.minz,
                rd.aabb.maxx,
                rd.aabb.maxy,
                rd.aabb.maxz,
            ];
            if !segment_hits_aabb(ox, oy, oz, dx, dy, dz, best, ab, 0.2) {
                continue;
            }
            for i in 0..rd.bone_count {
                let a = rd.bone_head[i] as usize;
                let c = rd.bone_tail[i] as usize;
                let t = ray_capsule(
                    ox,
                    oy,
                    oz,
                    dx,
                    dy,
                    dz,
                    rd.px[a],
                    rd.py[a],
                    rd.pz[a],
                    rd.px[c],
                    rd.py[c],
                    rd.pz[c],
                    f64::from(rd.bone_radius[i]),
                    best,
                );
                if t < 0.0 || t >= best {
                    continue;
                }
                best = t;
                out.hit = true;
                out.distance = t;
                out.point = [ox + dx * t, oy + dy * t, oz + dz * t];
                let cl = closest_pt_seg_seg(
                    out.point[0],
                    out.point[1],
                    out.point[2],
                    out.point[0],
                    out.point[1],
                    out.point[2],
                    rd.px[a],
                    rd.py[a],
                    rd.pz[a],
                    rd.px[c],
                    rd.py[c],
                    rd.pz[c],
                );
                let mut n = [
                    out.point[0] - cl.bx,
                    out.point[1] - cl.by,
                    out.point[2] - cl.bz,
                ];
                let len_sq = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
                if len_sq < 1e-12 {
                    n = [-dx, -dy, -dz];
                } else {
                    let l0 = len_sq.sqrt();
                    let l = if l0 == 0.0 || l0.is_nan() { 1.0 } else { l0 };
                    n = [n[0] / l, n[1] / l, n[2] / l];
                }
                out.normal = n;
                out.surface = Surface::Flesh;
                out.ragdoll = Some(rd.id);
                out.object = entry.actor.map_or(HitObject::None, HitObject::Actor);
                out.actor = entry.actor;
                out.part = rd.spec.get(i).map(|s| s.name.clone());
                out.collider = None;
                out.body = None;
                out.triangle = -1;
            }
        }
        best
    }

    /// Cheap occlusion test — statics only, no ordering, no record.
    /// `index.js:597-613`.
    pub fn raycast_any(
        &mut self,
        origin: [f64; 3],
        dir: [f64; 3],
        max_dist: f64,
        mask: u16,
    ) -> bool {
        let (dx, dy, dz) = (dir[0], dir[1], dir[2]);
        let l = crate::jsmath::hypot3(dx, dy, dz);
        if l < 1e-9 {
            return false;
        }
        self.ray_count += 1;
        self.static_world.raycast_any(
            origin[0],
            origin[1],
            origin[2],
            dx / l,
            dy / l,
            dz / l,
            max_dist,
            mask,
        )
    }

    /// True when nothing blocks the straight line between two points.
    /// `index.js:616-623`. Note it calls `staticWorld.raycastAny` directly and
    /// therefore does **not** bump `stats.raycasts`, unlike
    /// [`PhysicsCore::raycast_any`].
    pub fn line_of_sight(&self, from: [f64; 3], to: [f64; 3], mask: u16) -> bool {
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let dz = to[2] - from[2];
        let d = crate::jsmath::hypot3(dx, dy, dz);
        if d < 1e-6 {
            return true;
        }
        !self.static_world.raycast_any(
            from[0],
            from[1],
            from[2],
            dx / d,
            dy / d,
            dz / d,
            d - 1e-3,
            mask,
        )
    }

    /// `sphereCast`. `index.js:625-627` — a capsule cast with a zero-length
    /// segment.
    pub fn sphere_cast(
        &mut self,
        origin: [f64; 3],
        dir: [f64; 3],
        radius: f64,
        max_dist: f64,
        mask: u16,
    ) -> Hit {
        self.capsule_cast(origin, origin, radius, dir, max_dist, mask)
    }

    /// `capsuleCast`. `index.js:629-655`. Statics only.
    #[allow(clippy::too_many_arguments)]
    pub fn capsule_cast(
        &mut self,
        p0: [f64; 3],
        p1: [f64; 3],
        radius: f64,
        dir: [f64; 3],
        max_dist: f64,
        mask: u16,
    ) -> Hit {
        let mut out = Hit::default();
        let (mut dx, mut dy, mut dz) = (dir[0], dir[1], dir[2]);
        let l = crate::jsmath::hypot3(dx, dy, dz);
        if l < 1e-9 {
            return out;
        }
        dx /= l;
        dy /= l;
        dz /= l;
        self.ray_count += 1;
        let raw = self.static_world.sweep_capsule(
            p0[0], p0[1], p0[2], p1[0], p1[1], p1[2], radius, dx, dy, dz, max_dist, mask,
        );
        if raw.hit {
            out.hit = true;
            out.distance = raw.t;
            out.fraction = raw.t / max_dist;
            out.point = [raw.px, raw.py, raw.pz];
            out.normal = [raw.nx, raw.ny, raw.nz];
            out.surface = Surface::ALL[raw.surface as usize];
            out.triangle = raw.tri;
            out.object = HitObject::None;
        } else {
            out.distance = max_dist;
            out.point = [
                p0[0] + dx * max_dist,
                p0[1] + dy * max_dist,
                p0[2] + dz * max_dist,
            ];
            out.normal = [-dx, -dy, -dz];
        }
        out
    }

    /// Contact count; the contacts themselves come back with it (the source
    /// leaves them in `physics.staticWorld.contacts`). `index.js:658-662`.
    pub fn overlap_capsule(
        &self,
        p0: [f64; 3],
        p1: [f64; 3],
        radius: f64,
        mask: u16,
    ) -> Contacts {
        self.static_world
            .overlap_capsule(p0[0], p0[1], p0[2], p1[0], p1[1], p1[2], radius, mask, 0.0)
    }

    /// `checkCapsule`. `index.js:664-666` — true when clear.
    pub fn check_capsule(&self, p0: [f64; 3], p1: [f64; 3], radius: f64, mask: u16) -> bool {
        self.overlap_capsule(p0, p1, radius, mask).count() == 0
    }

    /// `overlapSphere`. `index.js:668-672`.
    pub fn overlap_sphere(&self, center: [f64; 3], radius: f64, mask: u16) -> Contacts {
        self.static_world.overlap_capsule(
            center[0], center[1], center[2], center[0], center[1], center[2], radius, mask, 0.0,
        )
    }

    /// Floor height under `(x, z)`, or `-Infinity` when there is no floor.
    /// `index.js:675-678`. The source's defaults are `fromY = 200`,
    /// `mask = MASK.WORLD`.
    pub fn ground_height(&mut self, x: f64, z: f64, from_y: f64, mask: u16) -> f64 {
        let h = self.raycast([x, from_y, z], [0.0, -1.0, 0.0], 1000.0, mask);
        if h.hit {
            h.point[1]
        } else {
            f64::NEG_INFINITY
        }
    }

    /* ---------------------------------------------------------------- */
    /* Characters                                                        */
    /* ---------------------------------------------------------------- */

    /// `createCharacter(opts)`. `index.js:684-692`. Returns the controller's
    /// index in the registry — the source returns the object itself and keeps
    /// a reference; an index is the same association without aliasing.
    pub fn create_character(&mut self, opts: CharacterOpts) -> usize {
        let c = Character::new(Rc::clone(&self.static_world), opts);
        self.characters.push(c);
        self.characters.len() - 1
    }

    /// `removeCharacter(c)`. `index.js:694-697`. Later indices shift down, as
    /// the source's `splice` does.
    pub fn remove_character(&mut self, index: usize) {
        if index < self.characters.len() {
            self.characters.remove(index);
        }
    }

    pub fn character(&self, i: usize) -> Option<&Character> {
        self.characters.get(i)
    }

    pub fn character_mut(&mut self, i: usize) -> Option<&mut Character> {
        self.characters.get_mut(i)
    }

    pub fn character_count(&self) -> usize {
        self.characters.len()
    }

    /* ---------------------------------------------------------------- */
    /* Ballistics                                                        */
    /* ---------------------------------------------------------------- */

    /// Trace a round through the world, penetrating what it can, and emit
    /// `bullet:impact` for every entry and every exit. `index.js:708-714`.
    ///
    /// **Divergence, and it is `penetration.rs`'s, not this file's.** The
    /// source's `Ballistics.fire` calls `phys.raycast` — the *facade* raycast,
    /// which sees colliders, rigid bodies and ragdolls — and calls
    /// `phys.emitImpact` from inside its own loop, interleaved with the trace.
    /// `physics/penetration.rs` instead raycasts `StaticWorld` directly and
    /// emits nothing. So here the round sees only static geometry, and the
    /// impacts are emitted after the trace completes rather than during it.
    /// The emitted *sequence* is identical either way (nothing in this port
    /// mutates the world from a `bullet:impact` handler); what is genuinely
    /// lost is dynamic-body penetration. Fixing it means widening
    /// `Ballistics` to take the facade, which is that slice's file.
    pub fn fire_bullet(&mut self, opts: BulletOpts) -> Vec<Impact> {
        let rng = Rc::clone(&self.rng);
        // `&*self.static_world` — the STATIC world, which is today's behaviour and not
        // the source's. `penetration.js` passes `this.phys`, the facade, so a
        // bullet can hit a collider, a rigid body or a ragdoll; this can only
        // hit static geometry, so `damage:dealt` never fires from a shot.
        //
        // The seam now exists: `Ballistics::fire` takes a `&dyn RayWorld` per
        // call rather than holding one, so switching this to the facade is a
        // one-line change. What blocks it is that `PhysicsSystem::raycast` takes
        // `&mut self` and returns `Hit` rather than `HitRecord`, so the facade
        // cannot implement `RayWorld` as written — that is a change to this
        // file's own query surface, not to the solver.
        let world: &dyn crate::physics::penetration::RayWorld = &*self.static_world;
        let n = self.ballistics.fire(
            world,
            opts.origin,
            opts.dir,
            opts.max_dist,
            opts.damage,
            opts.penetration,
            opts.mask,
            opts.dropoff,
            Some(rng),
            true,
        );
        let raw: Vec<BallisticImpact> = self.ballistics.impacts()[..n].to_vec();

        // The incident direction the source hands `emitImpact` is the
        // *normalised* ray direction at the moment of that layer's hit. Only
        // the first layer's is recoverable from the impact list (later layers
        // may have deflected), so it is recomputed per impact from the segment
        // the round actually travelled — see the notes file.
        let dl = crate::jsmath::hypot3(opts.dir[0], opts.dir[1], opts.dir[2]);
        let base_dir = if dl > 0.0 {
            [opts.dir[0] / dl, opts.dir[1] / dl, opts.dir[2] / dl]
        } else {
            opts.dir
        };

        let mut prev = opts.origin;
        let mut out = Vec::with_capacity(raw.len());
        for (i, im) in raw.iter().enumerate() {
            let d = [
                im.point[0] - prev[0],
                im.point[1] - prev[1],
                im.point[2] - prev[2],
            ];
            let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let incident = if i == 0 || l <= 0.0 {
                base_dir
            } else {
                [d[0] / l, d[1] / l, d[2] / l]
            };
            prev = im.point;
            out.push(self.emit_impact(
                im.point,
                im.normal,
                incident,
                im.surface,
                im.damage,
                im.exit,
                None,
            ));
        }
        out
    }

    /// `emitImpact(...)`. `index.js:716-741`. Builds the impact record, emits
    /// `bullet:impact`, and — on an entry that struck an actor — `damage:dealt`.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_impact(
        &mut self,
        point: [f64; 3],
        normal: [f64; 3],
        incident: [f64; 3],
        surface: Surface,
        damage: f64,
        exit: bool,
        hit: Option<&Hit>,
    ) -> Impact {
        let p = Impact {
            point,
            normal,
            incident,
            surface,
            damage,
            exit,
            object: hit.map_or(HitObject::None, |h| h.object),
            body: hit.and_then(|h| h.body),
            actor: hit.and_then(|h| h.actor),
            part: hit.and_then(|h| h.part.clone()),
        };

        if let Some(events) = self.events.as_ref() {
            // See "Events" in the module doc: the payload that crosses the bus
            // is the existing `audio` fork, which carries four of the eleven
            // fields. The full record is the return value.
            events.emit(
                "bullet:impact",
                &crate::audio::system::BulletImpact {
                    point: Some(p.point),
                    surface: Some(p.surface),
                    damage: Some(p.damage),
                    exit: p.exit,
                },
            );

            if p.actor.is_some() && !exit {
                let scale = hit.map_or(1.0, |h| {
                    h.collider
                        .and_then(|id| self.colliders.iter().find(|c| c.id == id))
                        .map_or(1.0, |c| c.damage_scale)
                });
                events.emit(
                    "damage:dealt",
                    &crate::ui::system::DamageDealt {
                        has_target: true,
                        target_is_player: false,
                        target_name: None,
                        name: None,
                        headshot: hit.and_then(|h| h.part.as_deref()) == Some("head"),
                        armour: false,
                        killed: false,
                        amount: Some(damage * scale),
                        point: Some(p.point),
                    },
                );
            }
        }
        p
    }

    /// Radial blast: shoves rigid bodies and ragdolls, occluded by the world
    /// so a grenade behind a wall doesn't throw the crate in front of it.
    /// `index.js:747-766`.
    pub fn explode(&mut self, e: Explosion) {
        let pos = e.position;
        let radius = e.radius.unwrap_or(5.0);
        let strength = e.impulse.unwrap_or_else(|| e.damage.unwrap_or(100.0) * 0.9);
        self.bodies
            .apply_radial_impulse(pos[0], pos[1], pos[2], radius, strength * 0.06);

        let world = Rc::clone(&self.static_world);
        for entry in &mut self.ragdolls {
            let rd = &mut entry.doll;
            let cx = (rd.aabb.minx + rd.aabb.maxx) * 0.5;
            let cy = (rd.aabb.miny + rd.aabb.maxy) * 0.5;
            let cz = (rd.aabb.minz + rd.aabb.maxz) * 0.5;
            let dx = cx - pos[0];
            let dy = cy - pos[1];
            let dz = cz - pos[2];
            let d = crate::jsmath::hypot3(dx, dy, dz);
            if d > radius {
                continue;
            }
            // `lineOfSight`, inlined so the shared `static_world` borrow does
            // not collide with the `&mut` on the ragdoll list.
            let ld = crate::jsmath::hypot3(cx - pos[0], cy - pos[1], cz - pos[2]);
            let visible = ld < 1e-6
                || !world.raycast_any(
                    pos[0],
                    pos[1],
                    pos[2],
                    (cx - pos[0]) / ld,
                    (cy - pos[1]) / ld,
                    (cz - pos[2]) / ld,
                    ld - 1e-3,
                    mask::EXPLOSION,
                );
            if !visible {
                continue;
            }
            let f = (1.0 - d / radius) * strength * 0.5;
            // `1 / (d || 1e-4)` — JS falsiness, so a zero *or* NaN distance
            // takes the substitute. `jsmath::or_one` is the same shape with a
            // different substitute; this one is spelled out rather than
            // generalised, so `jsmath` keeps owning exactly the builtins.
            let inv = 1.0 / if d == 0.0 || d.is_nan() { 1e-4 } else { d };
            rd.apply_impulse(
                pos[0],
                pos[1],
                pos[2],
                dx * inv * f,
                dy * inv * f + f * 0.4,
                dz * inv * f,
                radius,
                RAGDOLL_DT,
            );
        }
    }

    /* ---------------------------------------------------------------- */
    /* Rigid bodies                                                      */
    /* ---------------------------------------------------------------- */

    /// `addRigidBody(opts)`. `index.js:772-777`. Returns the body id.
    pub fn add_rigid_body(&mut self, opts: RigidBodyOpts) -> i32 {
        let he = opts.half_extents.unwrap_or([0.1, 0.1, 0.1]);
        let (hx, hy, hz) = (he[0], he[1], he[2]);
        let radius = opts.radius.unwrap_or_else(|| hx.min(hy).min(hz));
        let half_height = opts.half_height.unwrap_or_else(|| (hy - radius).max(0.0));
        let mass = opts.mass.unwrap_or(1.0);

        // `opts.surfaceType` is applied *after* construction (`index.js:774`),
        // so it wins over `opts.surface`.
        let surface = opts
            .surface_type
            .as_deref()
            .map(|s| surface_index(s, Surface::Concrete))
            .or(opts.surface)
            .unwrap_or(Surface::Concrete);

        let body = RigidBody::new(
            0,
            opts.shape.unwrap_or(Shape::Box),
            hx,
            hy,
            hz,
            radius,
            half_height,
            mass,
            opts.position.unwrap_or([0.0; 3]),
            opts.quaternion.unwrap_or([0.0, 0.0, 0.0, 1.0]),
            opts.velocity.unwrap_or([0.0; 3]),
            opts.angular_velocity.unwrap_or([0.0; 3]),
            opts.restitution.unwrap_or(0.25),
            opts.friction.unwrap_or(0.6),
            opts.linear_damping.unwrap_or(0.16),
            opts.angular_damping.unwrap_or(0.5),
            opts.gravity_scale.unwrap_or(1.0),
            surface.index(),
            opts.mask.unwrap_or(mask::DEBRIS),
            opts.layer.unwrap_or(0),
            opts.ccd.unwrap_or(true),
            opts.lifetime.unwrap_or(f64::INFINITY),
        );
        let added = self.bodies.add(body);
        if let Some(o) = opts.object3d {
            self.render_objects.insert(added.id, o);
        }
        added.id
    }

    /// `removeRigidBody(b)`. `index.js:779-781`.
    pub fn remove_rigid_body(&mut self, id: i32) {
        self.bodies.remove(id);
        self.render_objects.remove(&id);
    }

    /// Associate a render object with a body — the source's
    /// `body.object3D = o`. See seam 4.
    pub fn set_body_object(&mut self, id: i32, object: ObjectId) {
        self.render_objects.insert(id, object);
    }

    /// Convenience for fx: a tumbling chunk with sensible defaults.
    /// `index.js:784-804`.
    ///
    /// Draw order is part of the contract: the three `signed()` calls that set
    /// the angular velocity happen **after** the body is added.
    pub fn spawn_debris(
        &mut self,
        position: [f64; 3],
        velocity: [f64; 3],
        opts: DebrisOpts,
    ) -> i32 {
        let s = opts.size.unwrap_or(0.08);
        let si = surface_index(opts.surface.as_deref().unwrap_or("concrete"), Surface::Concrete);
        let props = SURFACE_PROPS[si.index() as usize];
        let id = self.add_rigid_body(RigidBodyOpts {
            shape: Some(opts.shape.unwrap_or(Shape::Box)),
            half_extents: Some([s, s * 0.7, s * 0.85]),
            radius: Some(s),
            mass: Some(
                opts.mass
                    .unwrap_or_else(|| (0.01_f64).max(s * s * s * 4.0 * props.density)),
            ),
            position: Some(position),
            velocity: Some(velocity),
            restitution: Some(opts.restitution.unwrap_or(props.restitution)),
            friction: Some(opts.friction.unwrap_or(props.friction)),
            lifetime: Some(opts.lifetime.unwrap_or(20.0)),
            surface: Some(si),
            object3d: opts.object3d,
            ..RigidBodyOpts::default()
        });
        let (ax, ay, az) = {
            let mut r = self.rng.borrow_mut();
            (r.signed() * 14.0, r.signed() * 14.0, r.signed() * 14.0)
        };
        if let Some(b) = self.bodies.get_body_mut(id) {
            b.angular_velocity = [ax, ay, az];
        }
        id
    }

    /* ---------------------------------------------------------------- */
    /* Ragdolls                                                          */
    /* ---------------------------------------------------------------- */

    /// `createRagdoll(opts)`. `index.js:810-825`. Evicts the oldest doll
    /// while the list is at `max_ragdolls`. Returns the new doll's id.
    pub fn create_ragdoll(&mut self, spawn: RagdollSpawn) -> i32 {
        // `while (this.ragdolls.length >= this.maxRagdolls) shift()?.dispose()`
        // (`index.js:811-813`). With `maxRagdolls === 0` the source spins
        // forever — `shift()` returns `undefined`, `?.` swallows it, the
        // condition never changes. The `is_empty` guard is a deliberate,
        // documented divergence: an infinite loop is not behaviour to port,
        // and `maxRagdolls` is 8.
        while self.ragdolls.len() >= self.max_ragdolls && !self.ragdolls.is_empty() {
            let mut e = self.ragdolls.remove(0);
            e.doll.dispose();
        }
        let mut opts = spawn.opts;
        // `{ gravity: this.gravity, ...opts }` — the caller's gravity wins.
        if opts.gravity.is_none() {
            opts.gravity = Some(self.gravity);
        }
        let mut rd = Ragdoll::new(Some(Rc::clone(&self.static_world)), opts);
        if let Some(v) = spawn.velocity {
            rd.set_velocity(v[0], v[1], v[2], RAGDOLL_DT);
        }
        if let (Some(i), Some(p)) = (spawn.impulse, spawn.point) {
            rd.apply_impulse(
                p[0],
                p[1],
                p[2],
                i[0],
                i[1],
                i[2],
                spawn.impulse_radius.unwrap_or(0.45),
                RAGDOLL_DT,
            );
        }
        let id = rd.id;
        self.ragdolls.push(RagdollEntry {
            doll: rd,
            actor: spawn.actor,
        });
        id
    }

    /// `removeRagdoll(rd)`. `index.js:842-846`.
    pub fn remove_ragdoll(&mut self, id: i32) {
        if let Some(i) = self.ragdolls.iter().position(|e| e.doll.id == id) {
            let mut e = self.ragdolls.remove(i);
            e.doll.dispose();
        }
    }

    pub fn ragdoll(&self, id: i32) -> Option<&Ragdoll> {
        self.ragdolls
            .iter()
            .find(|e| e.doll.id == id)
            .map(|e| &e.doll)
    }

    pub fn ragdoll_mut(&mut self, id: i32) -> Option<&mut Ragdoll> {
        self.ragdolls
            .iter_mut()
            .find(|e| e.doll.id == id)
            .map(|e| &mut e.doll)
    }

    /// Every live ragdoll, in creation order.
    pub fn ragdoll_ids(&self) -> Vec<i32> {
        self.ragdolls.iter().map(|e| e.doll.id).collect()
    }

    pub fn ragdoll_at(&self, i: usize) -> Option<&Ragdoll> {
        self.ragdolls.get(i).map(|e| &e.doll)
    }

    /* ---------------------------------------------------------------- */
    /* Dynamic colliders / hitboxes                                      */
    /* ---------------------------------------------------------------- */

    /// `addCollider(opts)`. `index.js:871-875`. Returns the collider id.
    pub fn add_collider(&mut self, opts: ColliderOpts) -> u32 {
        let id = self.next_collider_id;
        self.next_collider_id += 1;
        self.colliders.push(Collider::new(id, opts));
        id
    }

    /// `removeCollider(c)`. `index.js:877-880`.
    pub fn remove_collider(&mut self, id: u32) {
        if let Some(i) = self.colliders.iter().position(|c| c.id == id) {
            self.colliders.remove(i);
        }
    }

    pub fn collider(&self, id: u32) -> Option<&Collider> {
        self.colliders.iter().find(|c| c.id == id)
    }

    pub fn collider_mut(&mut self, id: u32) -> Option<&mut Collider> {
        self.colliders.iter_mut().find(|c| c.id == id)
    }

    pub fn colliders(&self) -> &[Collider] {
        &self.colliders
    }

    /* ---------------------------------------------------------------- */
    /* Frame                                                             */
    /* ---------------------------------------------------------------- */

    /// `fixedUpdate(h, ctx)`. `index.js:886-913`, minus the
    /// `performance.now()` bracket and the `?physdemo=1` spawn hook.
    ///
    /// The auto-rescan timer is kept even though the rescan itself cannot fire
    /// (there is no scene to traverse and the world is immutable — seam 1):
    /// it is one addition and a comparison, and deleting it would quietly
    /// change what a future agent has to re-derive when mesh baking lands.
    pub fn fixed_update(&mut self, h: f64) {
        if self.explicit_statics == 0 {
            self.auto_scan_timer += h;
            if self.auto_scan_timer > AUTO_SCAN_PERIOD {
                self.auto_scan_timer = 0.0;
                // `_ensureStatics(false)`: `meshCount === this._lastMeshCount`
                // (both 0) so the source returns immediately.
            }
        }
        // `if (this.staticWorld.dirty) { build(); _syncStats(); }` — the world
        // is built once in `new` and never re-dirtied here.
        if self.static_world.dirty() {
            self.sync_stats();
        }

        self.bodies.step(h);
        for e in &mut self.ragdolls {
            e.doll.step(h);
        }

        self.stats.awake = self.bodies.awake_count();
        self.stats.raycasts = self.ray_count;
        self.ray_count = 0;
    }

    /// `update(dt, ctx)`. `index.js:915-932` — interpolate rigid bodies into
    /// their render transforms using the engine's physics alpha, so debris
    /// never strobes when the frame rate dips.
    pub fn update(&mut self, alpha: f64) -> Vec<InterpolatedPose> {
        let mut out = Vec::new();
        for b in self.bodies.bodies() {
            let Some(object) = self.render_objects.get(&b.id).copied() else {
                continue;
            };
            let (position, quaternion) = if b.sleeping {
                (b.position, b.quaternion)
            } else {
                (
                    lerp_vectors(b.prev_position, b.position, alpha),
                    slerp(b.prev_quaternion, b.quaternion, alpha),
                )
            };
            out.push(InterpolatedPose {
                object,
                body: b.id,
                position,
                quaternion,
            });
        }
        out
    }

    /// `lateUpdate(dt, ctx)`. `index.js:934-944`. `rd.writeToSkeleton()` is
    /// absent for the same reason `_handleDeath` is — see the module doc.
    pub fn late_update(&mut self, dt: f64, camera: Option<[f64; 3]>) {
        let enabled = self.debug.as_ref().is_some_and(|d| d.enabled);
        if enabled {
            let views: Vec<RagdollView<'_>> =
                self.ragdolls.iter().map(|e| RagdollView(&e.doll)).collect();
            let ragdolls: Vec<&dyn RagdollBones> =
                views.iter().map(|v| v as &dyn RagdollBones).collect();
            let colliders: Vec<&dyn DebugCollider> = self
                .colliders
                .iter()
                .map(|c| c as &dyn DebugCollider)
                .collect();
            let scene = DebugScene {
                static_world: &self.static_world,
                camera,
                characters: &self.characters,
                bodies: self.bodies.bodies(),
                ragdolls: &ragdolls,
                colliders: &colliders,
            };
            if let Some(d) = self.debug.as_mut() {
                d.rebuild(&scene, dt);
            }
        }
        self.stats.bodies = self.bodies.bodies().len();
        self.stats.ragdolls = self.ragdolls.len();
        self.stats.characters = self.characters.len();
        self.stats.colliders = self.colliders.len();
    }

    /// `_syncStats()`. `index.js:946-967`, minus the `console.info` line —
    /// the Module Law bans console output outside tests, and the line exists
    /// to tell *another agent* whether their geometry reached collision, which
    /// [`PhysicsCore::stats`] already answers as a value.
    pub fn sync_stats(&mut self) {
        self.stats.triangles = self.static_world.tri_count();
        self.stats.nodes = self.static_world.node_count();
        // `for (const o of objects) if (o && o.alive) n++`.
        self.stats.objects = self.live_objects;
    }

    /* ---------------------------------------------------------------- */
    /* Debug                                                             */
    /* ---------------------------------------------------------------- */

    /// `setDebugDraw(on, opts)`. `index.js:978-985`.
    pub fn set_debug_draw(
        &mut self,
        on: bool,
        triangles: Option<bool>,
        nodes: Option<bool>,
        rays: Option<bool>,
        radius: Option<f64>,
    ) {
        let Some(d) = self.debug.as_mut() else {
            return;
        };
        if let Some(v) = triangles {
            d.show_triangles = v;
        }
        if let Some(v) = nodes {
            d.show_nodes = v;
        }
        if let Some(v) = rays {
            d.show_rays = v;
        }
        if let Some(v) = radius {
            d.radius = v;
        }
        d.set_enabled(on);
    }

    /// `toggleDebugDraw()`. `index.js:987-990`.
    pub fn toggle_debug_draw(&mut self) -> bool {
        let on = !self.debug.as_ref().is_some_and(|d| d.enabled);
        self.set_debug_draw(on, None, None, None, None);
        self.debug.as_ref().is_some_and(|d| d.enabled)
    }

    /// `debugState(name)`. `index.js:993-999`.
    pub fn debug_state(&mut self, name: &str, camera: Option<([f64; 3], [f64; 4])>) -> PhysicsStats {
        match name {
            "collision" => self.set_debug_draw(true, Some(true), Some(false), None, None),
            "bvh" => self.set_debug_draw(true, Some(false), Some(true), None, None),
            "demo" => {
                if let Some((p, q)) = camera {
                    self.spawn_demo(p, q);
                }
            }
            "off" => self.set_debug_draw(false, None, None, None, None),
            _ => {}
        }
        self.stats
    }

    /// `_spawnDemo()`. `index.js:1006-1026`. Purely a verification aid; nothing
    /// in the game calls it. It is ported because it is the tightest statement
    /// of the rng draw-order contract in the file — nine draws per chunk, in
    /// strict left-to-right argument order — and that order is exactly what a
    /// port silently gets wrong.
    ///
    /// The source reads `ctx.camera`; there is no camera at this tier, so its
    /// world pose is passed in.
    pub fn spawn_demo(&mut self, cam_pos: [f64; 3], cam_quat: [f64; 4]) -> PhysicsStats {
        self.set_debug_draw(true, Some(true), None, None, Some(30.0));
        // `_v.set(0, 0, -1).applyQuaternion(cam.quaternion)`.
        let fwd = apply_quaternion([0.0, 0.0, -1.0], cam_quat);
        let cx = cam_pos[0] + fwd[0] * 6.0;
        let cz = cam_pos[2] + fwd[2] * 6.0;
        let floor = self.ground_height(cx, cz, cam_pos[1] + 20.0, mask::WORLD);
        let base = if floor.is_finite() { floor } else { 0.0 };

        for i in 0..14 {
            // Argument evaluation is left to right, and the object literals are
            // evaluated before `spawnDebris` runs: x, z, then vx, vz, then
            // size, then the surface pick — six draws, before the three inside
            // `spawn_debris`.
            let (px, pz, vx, vz, size, surf) = {
                let mut r = self.rng.borrow_mut();
                let px = cx + r.signed() * 1.6;
                let pz = cz + r.signed() * 1.6;
                let vx = r.signed() * 2.0;
                let vz = r.signed() * 2.0;
                let size = 0.09 + r.float() * 0.06;
                let surf = *r.pick(&["concrete", "wood", "metal"]);
                (px, pz, vx, vz, size, surf)
            };
            self.spawn_debris(
                [px, base + 1.2 + f64::from(i) * 0.22, pz],
                [vx, 0.0, vz],
                DebrisOpts {
                    size: Some(size),
                    surface: Some(surf.to_string()),
                    lifetime: Some(1e9),
                    ..DebrisOpts::default()
                },
            );
        }

        let m = translation(cx + 1.5, base + 1.15, cz);
        let id = self.create_ragdoll(RagdollSpawn {
            opts: RagdollOpts {
                transform: Some(m),
                height: Some(1.82),
                mass: Some(84.0),
                ..RagdollOpts::default()
            },
            ..RagdollSpawn::default()
        });
        if let Some(rd) = self.ragdoll_mut(id) {
            rd.set_velocity(-1.2, 0.2, 0.4, RAGDOLL_DT);
        }
        self.stats
    }

    /// `dispose()`. `index.js:1028-1039`.
    pub fn dispose(&mut self) {
        self.debug = None;
        self.bodies.clear();
        for e in &mut self.ragdolls {
            e.doll.dispose();
        }
        self.ragdolls.clear();
        self.characters.clear();
        self.colliders.clear();
        self.render_objects.clear();
    }
}

/// `_colliderNormal(c, point, outN, dx, dy, dz)`. `index.js:495-515`.
///
/// **`Math.sign(v) || 1`.** `Math.sign(0)` is `0` and `Math.sign(-0)` is `-0`,
/// both falsy, so the `|| 1` turns a dead-centre face hit into `+1`.
/// `f64::signum` would return `1.0` for `+0.0` and **`-1.0`** for `-0.0` and
/// flip the normal on a face the ray struck exactly through its axis.
fn collider_normal(c: &Collider, point: [f64; 3], dx: f64, dy: f64, dz: f64) -> [f64; 3] {
    let mut n = if c.shape == ColliderShape::Box {
        let v = apply_matrix4(point, &c.inverse);
        let ax = v[0].abs() / c.hx;
        let ay = v[1].abs() / c.hy;
        let az = v[2].abs() / c.hz;
        let axis = if ax >= ay && ax >= az {
            [sign_or_one(v[0]), 0.0, 0.0]
        } else if ay >= az {
            [0.0, sign_or_one(v[1]), 0.0]
        } else {
            [0.0, 0.0, sign_or_one(v[2])]
        };
        transform_direction(axis, &c.matrix)
    } else {
        let cl = closest_pt_seg_seg(
            point[0], point[1], point[2], point[0], point[1], point[2], c.ax, c.ay, c.az, c.bx,
            c.by, c.bz,
        );
        let m = [point[0] - cl.bx, point[1] - cl.by, point[2] - cl.bz];
        let len_sq = m[0] * m[0] + m[1] * m[1] + m[2] * m[2];
        if len_sq < 1e-12 {
            [-dx, -dy, -dz]
        } else {
            let l0 = len_sq.sqrt();
            let l = if l0 == 0.0 || l0.is_nan() { 1.0 } else { l0 };
            [m[0] / l, m[1] / l, m[2] / l]
        }
    };
    if n[0] * dx + n[1] * dy + n[2] * dz > 0.0 {
        n = [-n[0], -n[1], -n[2]];
    }
    n
}

/// `Math.sign(v) || 1`.
fn sign_or_one(v: f64) -> f64 {
    let s = crate::jsmath::sign(v);
    // JS falsiness: `0`, `-0` and `NaN` all take the `|| 1`.
    if s == 0.0 || s.is_nan() {
        1.0
    } else {
        s
    }
}

/// `Vector3.applyQuaternion(q)`, transcribed from `three@0.180`.
fn apply_quaternion(v: [f64; 3], q: [f64; 4]) -> [f64; 3] {
    let (x, y, z) = (v[0], v[1], v[2]);
    let (qx, qy, qz, qw) = (q[0], q[1], q[2], q[3]);
    let tx = 2.0 * (qy * z - qz * y);
    let ty = 2.0 * (qz * x - qx * z);
    let tz = 2.0 * (qx * y - qy * x);
    [
        x + qw * tx + qy * tz - qz * ty,
        y + qw * ty + qz * tx - qx * tz,
        z + qw * tz + qx * ty - qy * tx,
    ]
}

/// `Matrix4.makeTranslation(x, y, z)` — column-major elements.
fn translation(x: f64, y: f64, z: f64) -> [f64; 16] {
    let mut m = IDENTITY4;
    m[12] = x;
    m[13] = y;
    m[14] = z;
    m
}

/* ================================================================== */
/* Subsystem                                                          */
/* ================================================================== */

/// The registry-facing wrapper. `static id = 'physics'`, `static deps = []`.
pub struct PhysicsSystem {
    pub core: Rc<RefCell<PhysicsCore>>,
    offs: Vec<(&'static str, SubscriptionId)>,
}

impl PhysicsSystem {
    pub fn new(registry: StaticRegistry) -> Self {
        PhysicsSystem {
            core: Rc::new(RefCell::new(PhysicsCore::new(registry))),
            offs: Vec::new(),
        }
    }

    /// `init`'s event wiring (`index.js:248-251`), against the `Ctx` the
    /// registry hands `init`. The work is [`subscribe`]; this is the `Ctx`
    /// front door onto it.
    pub fn wire_events(&mut self, ctx: &Ctx<'_>) {
        self.offs.extend(subscribe(&self.core, ctx.events));
    }
}

/// `index.js:248-251`'s one subscription, taking the **bus** rather than a
/// [`Ctx`].
///
/// It was reachable only through `&Ctx<'_>`, and `Ctx` has a private field, so
/// nothing outside `crate::engine` can build one: any caller driving this core
/// without the subsystem registry (there is no `world` or `render` subsystem in
/// this port, so `Registry::resolve` cannot admit `player` at all) could not
/// subscribe. `ctx.events` was the only field the body ever read, so the
/// narrower parameter is also the honest one.
///
/// Only `explosion` is wired; `actor:death` is not — see the module doc.
pub fn subscribe(
    core: &Rc<RefCell<PhysicsCore>>,
    events: &crate::events::EventBus,
) -> Vec<(&'static str, SubscriptionId)> {
    let owned = Rc::clone(core);
    let id = events.on("explosion", move |p: &dyn Any| {
        if let Some(e) = p.downcast_ref::<crate::player::system::ExplosionEvent>() {
            owned.borrow_mut().explode(Explosion {
                position: e.position,
                radius: e.radius,
                // No existing `explosion` fork carries `impulse`; the
                // direct `explode()` call is the only way to set it.
                impulse: None,
                damage: e.damage,
            });
        }
        Ok(())
    });
    vec![("explosion", id)]
}

impl Subsystem for PhysicsSystem {
    fn id(&self) -> &'static str {
        "physics"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::FixedUpdate, Phase::Update, Phase::LateUpdate]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn init(&mut self, ctx: &Ctx<'_>) -> Result<(), CoreError> {
        let forked = ctx.rng.borrow_mut().fork();
        {
            let mut core = self.core.borrow_mut();
            core.set_rng(forked);
            core.set_events(ctx.events.clone());
        }
        self.wire_events(ctx);
        Ok(())
    }

    fn fixed_update(&mut self, h: Seconds, _ctx: &Ctx<'_>) {
        self.core.borrow_mut().fixed_update(f64::from(h.get()));
    }

    fn update(&mut self, _dt: Seconds, ctx: &Ctx<'_>) {
        self.core.borrow_mut().update(ctx.time.alpha);
    }

    fn late_update(&mut self, dt: Seconds, _ctx: &Ctx<'_>) {
        self.core
            .borrow_mut()
            .late_update(f64::from(dt.get()), None);
    }

    /// `dispose()`. The `offs` list exists so the subscriptions can be
    /// **cancelled**; clearing the `Vec` (what this used to do) drops the ids
    /// and leaves the handlers on the bus — each one holding an `Rc` to the
    /// core it was disposing, so the core outlived its own `dispose`. The same
    /// mistake is in `audio`, `ui`, `ai` and `weapons`, which are other slices'
    /// files.
    fn dispose(&mut self) {
        let bus = self.core.borrow().events.clone();
        for (name, id) in self.offs.drain(..) {
            // `None` only when `init` never ran, in which case nothing was
            // ever subscribed and `offs` is empty anyway.
            bus.iter().for_each(|b| b.off(name, id));
        }
        self.core.borrow_mut().dispose();
    }
}
