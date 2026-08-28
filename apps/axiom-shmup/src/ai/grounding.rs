//! Ported from Claude-of-Duty `src/ai/grounding.js:1-196`.
//!
//! `grounding.js` is a rendering system top to bottom: two
//! `THREE.InstancedMesh`es (`body`, `feet`), a `MeshBasicMaterial` pair, and a
//! `THREE.DataTexture` uploaded to the GPU. Everything in it that is *data or
//! arithmetic* is here; the GPU objects are not, because the engine has no
//! decal or billboard primitive to make them out of.
//!
//! What is ported:
//!
//! - [`build_texture`] — `buildTexture(size, power)` (`grounding.js:32-56`):
//!   the radial occlusion sprite's pixel data, a function of `(x, y)` alone.
//! - [`GroundShadows`] — `_place`/`addActor`'s placement math
//!   (`grounding.js:127-180`): the body-ellipse and per-foot contact-lobe
//!   transforms (position, facing, and the two-axis scale that *shrinks* a
//!   lifted foot's contact patch rather than fading it), collected per frame
//!   exactly as `begin()`/`addActor()`/`end()` do.
//! - [`Placement::instance_matrix`] — `_place`'s quaternion composition and
//!   `Matrix4.compose` (`grounding.js:129-133`), in Three's column-major
//!   `elements` order. Re-audited: an earlier pass called this part of the
//!   "upload" and dropped it, but composing the instance matrix is arithmetic,
//!   not a GPU call, and dropping it left the port's only non-trivial maths
//!   unported.
//! - [`surface`] — the material and draw flags the quads are drawn *with*
//!   (`grounding.js:72-87, 107-119`), recorded rather than left in the
//!   JavaScript for whoever wires the draw to re-derive.
//!
//! What remains out: `InstancedMesh.setMatrixAt` / `instanceMatrix.needsUpdate`
//! / `mesh.count` / `mesh.visible` (`grounding.js:133,137-144`) — the GPU
//! upload proper.
//!
//! ## Wired, but not yet drawn
//!
//! `addActor` reads a foot bone's world position via `agent.animator.bonePos`
//! (`grounding.js:159-162`); [`FootSource`] names exactly that one call, and
//! [`crate::ai::system::AiCore::late_update`] implements it against the real
//! [`crate::ai::animator::Animator`] and runs `begin()`/`add_actor()` every
//! frame. So the placements are real and current — and
//! [`crate::ai::system::AiCore::shadow_placements`] publishes them — but
//! **nothing consumes them to draw**, which is why the soldiers currently have
//! no contact shadow under them. That is the last step of this file's port and
//! it belongs with the other ground-projected effects, in the composing tier.

use crate::jsmath;

/// `agent.animator.bonePos(name, out)`, narrowed to the two bones
/// `addActor` reads (`grounding.js:160`, `const FEET = ['FootR', 'FootL']`).
/// `None` mirrors the source's `if (!Number.isFinite(this._foot.y)) continue;`
/// guard — a bone whose position is not yet valid this frame.
pub trait FootSource {
    fn foot_world(&self, foot: Foot) -> Option<[f64; 3]>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Foot {
    Right,
    Left,
}

/// Radial occlusion sprite: rgb white, alpha = occlusion. `buildTexture(size,
/// power)`. `grounding.js:32-56`. Row-major, 4 bytes (RGBA) per texel.
pub fn build_texture(size: usize, power: f64) -> Vec<u8> {
    let mut buf = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let u = ((x as f64 + 0.5) / size as f64) * 2.0 - 1.0;
            let v = ((y as f64 + 0.5) / size as f64) * 2.0 - 1.0;
            // `Math.hypot(u, v)` — V8 max-scales and Kahan-compensates, and
            // `f64::hypot` is a different implementation. See `crate::jsmath`.
            let r = jsmath::hypot2(u, v).min(1.0);
            let mut a = (-r * r * power).exp();
            a *= 1.0 - r * r * r; // hard zero at the rim: no visible disc edge
            let i = (y * size + x) * 4;
            buf[i] = 255;
            buf[i + 1] = 255;
            buf[i + 2] = 255;
            // `Math.round` breaks ties toward `+Infinity`, `f64::round` away
            // from zero.
            buf[i + 3] = jsmath::round(255.0 * a.clamp(0.0, 1.0)) as u8;
        }
    }
    buf
}

/// The default texture size/power pair for each sprite. `grounding.js:65-66`.
pub const TEXTURE_SIZE: usize = 64;
pub const BODY_POWER: f64 = 3.4;
pub const FOOT_POWER: f64 = 4.6;
/* ================================================================== */
/* The surface a contact shadow is drawn WITH                         */
/* ================================================================== */

/// `grounding.js:72-87` — the `MeshBasicMaterial` both instanced meshes share,
/// and `grounding.js:107-119`'s draw flags.
///
/// **These are here because the draw is not.** The engine has no decal or
/// billboard primitive, so nothing in this repo consumes
/// [`GroundShadows::end`]'s placements yet
/// (`crate::scene::wiring::soldier_draw` states the same, and names it as the
/// pooled camera-facing quad every other ground-projected effect in this port
/// also needs). Until that primitive lands, a port that carries only the
/// *placements* is carrying half of `grounding.js`: whoever wires the draw
/// would have to go back to the JavaScript for the surface, and the failure
/// modes of guessing it are specific and ugly — an additive blend paints grey
/// discs on the floor instead of darkening it, a tone-mapped colour drifts with
/// the exposure the shadow is supposed to be independent of, and a
/// depth-writing quad z-fights the road it lies on.
///
/// So they are recorded, in one place, next to the maths they belong to.
pub mod surface {
    /// `new THREE.Color(0.045, 0.05, 0.062)` — linear RGB, and deliberately
    /// **not** black: a contact shadow is lit by the sky it is occluding less
    /// of, so it keeps a trace of that sky's blue. Deep, but not a hole.
    pub const TINT: [f64; 3] = [0.045, 0.05, 0.062];

    /// The body ellipse's `opacity` (`grounding.js:86`).
    pub const BODY_OPACITY: f64 = 0.62;

    /// A foot lobe's `opacity` (`grounding.js:87`) — tighter and darker than
    /// the body's, because a sole in contact occludes far more of the sky than
    /// a pelvis 0.9 m above it.
    pub const FOOT_OPACITY: f64 = 0.85;

    /// `transparent: true, depthWrite: false, depthTest: true` — it lies *on*
    /// the road and must not write depth, or the road z-fights it.
    pub const DEPTH_WRITE: bool = false;
    pub const DEPTH_TEST: bool = true;

    /// `side: THREE.DoubleSide` — the quad is authored flat and yawed, so a
    /// camera below the floor plane must still see it rather than have it
    /// vanish.
    pub const DOUBLE_SIDED: bool = true;

    /// `toneMapped: false, fog: false` — the darkening is a fixed fraction of
    /// whatever is under it. Running it through the tone map would make the
    /// shadow's depth a function of the frame's exposure, and fogging it would
    /// lighten contact shadows in the distance, which is backwards.
    pub const TONE_MAPPED: bool = false;
    pub const FOGGED: bool = false;

    /// `renderOrder = 6` (`grounding.js:112`) — after the opaque world, before
    /// the FX smoke.
    pub const RENDER_ORDER: i32 = 6;

    /// `frustumCulled = false` (`grounding.js:109`): one instanced mesh holds
    /// every actor's quad, so its own bounds are meaningless.
    pub const FRUSTUM_CULLED: bool = false;

    /// `userData.owNoShadow` / `owNoPrepass` (`grounding.js:115-116`). The
    /// occlusion quads are the one thing in the frame that must never cast into
    /// the cascades — a shadow of a shadow — nor occlude the depth prepass.
    /// `owNoShadow` is the source's ONLY shadow-caster switch (the cascades draw
    /// with `scene.overrideMaterial` and never consult `mesh.castShadow`); see
    /// `apps/shmup/ARCHITECTURE.md:194-197`.
    pub const NO_SHADOW: bool = true;
    pub const NO_PREPASS: bool = true;

    /// `userData.owProbe` (`grounding.js:114`).
    pub const PROBE: bool = true;

    /// `capacity = Math.max(4, actors)` with `actors = 12` at the call site;
    /// the feet mesh is sized `capacity * 2` (`grounding.js:64, 89-90`).
    pub const DEFAULT_CAPACITY: usize = 12;
    pub const MIN_CAPACITY: usize = 4;
    pub const FEET_PER_ACTOR: usize = 2;
}


/// One occlusion quad's placement: world position (already lifted `+0.015 m`
/// off the floor, matching `_place`'s `y + 0.015`), the facing `yaw`, and the
/// ellipse's two radii. What a future rendering slice would `compose` into an
/// instance matrix (`_place`, `grounding.js:127-134`) and hand to
/// `InstancedMesh.setMatrixAt`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub position: [f64; 3],
    pub yaw: f64,
    pub rx: f64,
    pub rz: f64,
}

impl Placement {
    /// `_place`'s instance matrix (`grounding.js:129-133`): the quad's
    /// yaw-about-up composed with the lie-flat rotation, at the lifted
    /// position, scaled to the ellipse's diameters.
    ///
    /// Returned in **column-major** order — `THREE.Matrix4.elements`' layout,
    /// which is what `InstancedMesh.setMatrixAt` consumes. Writing this
    /// row-major would flip every off-diagonal sign and still compile.
    pub fn instance_matrix(&self) -> [f64; 16] {
        // `this._q.setFromAxisAngle(this._up, yaw)` — axis (0, 1, 0).
        let h = self.yaw / 2.0;
        let a = [0.0, h.sin(), 0.0, h.cos()];
        // `this._flat`, built once in the constructor
        // (`grounding.js:102`): axis (1, 0, 0), angle -PI/2.
        let fh = -std::f64::consts::PI / 2.0 / 2.0;
        let b = [fh.sin(), 0.0, 0.0, fh.cos()];
        // `.multiply(this._flat)` — `multiplyQuaternions(a, b)`, transcribed
        // term by term (float addition is not associative; do not tidy).
        let q = [
            a[0] * b[3] + a[3] * b[0] + a[1] * b[2] - a[2] * b[1],
            a[1] * b[3] + a[3] * b[1] + a[2] * b[0] - a[0] * b[2],
            a[2] * b[3] + a[3] * b[2] + a[0] * b[1] - a[1] * b[0],
            a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
        ];
        // `this._scale.set(rx * 2, rz * 2, 1)`
        let (sx, sy, sz) = (self.rx * 2.0, self.rz * 2.0, 1.0);
        // `Matrix4.compose(position, quaternion, scale)`
        let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, xy, xz) = (x * x2, x * y2, x * z2);
        let (yy, yz, zz) = (y * y2, y * z2, z * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);
        [
            (1.0 - (yy + zz)) * sx,
            (xy + wz) * sx,
            (xz - wy) * sx,
            0.0,
            (xy - wz) * sy,
            (1.0 - (xx + zz)) * sy,
            (yz + wx) * sy,
            0.0,
            (xz + wy) * sz,
            (yz - wx) * sz,
            (1.0 - (xx + yy)) * sz,
            0.0,
            self.position[0],
            self.position[1],
            self.position[2],
            1.0,
        ]
    }
}

/// `class GroundShadows`, placement math only. `grounding.js:58-193`.
pub struct GroundShadows {
    pub capacity: usize,
    body: Vec<Placement>,
    feet: Vec<Placement>,
}

impl GroundShadows {
    /// `constructor(parent, actors = 12)`. `grounding.js:63-105`, minus the
    /// mesh/material/texture construction.
    pub fn new(actors: usize) -> Self {
        GroundShadows {
            capacity: actors.max(4),
            body: Vec::new(),
            feet: Vec::new(),
        }
    }

    /// `begin()`. `grounding.js:122-125`.
    pub fn begin(&mut self) {
        self.body.clear();
        self.feet.clear();
    }

    /// `addActor(agent)`. `grounding.js:150-180`. `feet` is `None` for an
    /// actor with no bound animator yet — see [`FootSource`]'s doc comment.
    pub fn add_actor(&mut self, position: [f64; 3], yaw: f64, scale: f64, crouch: bool, feet: Option<&dyn FootSource>) {
        if !position[1].is_finite() {
            return;
        }
        let crouch_f = if crouch { 0.86 } else { 1.0 };
        if self.body.len() < self.capacity {
            self.body.push(Placement {
                position: lifted(position),
                yaw,
                rx: 0.44 * scale * crouch_f,
                rz: 0.34 * scale * crouch_f,
            });
        }
        let Some(feet_source) = feet else { return };
        for foot in [Foot::Right, Foot::Left] {
            if self.feet.len() >= self.capacity * 2 {
                break;
            }
            let Some(fp) = feet_source.foot_world(foot) else { continue };
            if !fp[1].is_finite() {
                continue;
            }
            // A boot 6 cm off the floor still darkens it; at 35 cm it does not.
            // The contact shrinks rather than fading, which is what a real one
            // does.
            let h = fp[1] - position[1];
            let k = 1.0 - ((h - 0.06) / 0.29).clamp(0.0, 1.0);
            if k <= 0.05 {
                continue;
            }
            self.feet.push(Placement {
                position: lifted([fp[0], position[1], fp[2]]),
                yaw,
                rx: 0.15 * scale * k,
                rz: 0.21 * scale * k,
            });
        }
    }

    /// `end()`. `grounding.js:137-144`, minus the GPU upload — the two
    /// placement lists collected since [`GroundShadows::begin`].
    pub fn end(&self) -> (&[Placement], &[Placement]) {
        (&self.body, &self.feet)
    }
}

fn lifted(p: [f64; 3]) -> [f64; 3] {
    [p[0], p[1] + 0.015, p[2]]
}
