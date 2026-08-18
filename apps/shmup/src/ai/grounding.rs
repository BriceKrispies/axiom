//! Ported from Claude-of-Duty `src/ai/grounding.js:1-196` — the pure math
//! only. `grounding.js` is a rendering system top to bottom: two
//! `THREE.InstancedMesh`es (`body`, `feet`), a `MeshBasicMaterial` pair, and a
//! `THREE.DataTexture` uploaded to the GPU. None of the engine's rendering
//! arm exists yet for this port to draw through (see
//! `docs/work-manifests/shmup-port/05-port-status.md`'s "Render
//! frame graph" item), so nothing here creates a mesh, a material or a
//! texture object.
//!
//! What *is* pure data/maths, and is ported faithfully:
//!
//! - [`build_texture`] — `buildTexture(size, power)` (`grounding.js:32-56`):
//!   the radial occlusion sprite's pixel data, a function of `(x, y)` alone.
//! - [`GroundShadows`] — `_place`/`addActor`'s placement math
//!   (`grounding.js:127-180`): the body-ellipse and per-foot contact-lobe
//!   transforms (position, facing, and the two-axis scale that *shrinks* a
//!   lifted foot's contact patch rather than fading it), collected per frame
//!   exactly as `begin()`/`addActor()`/`end()` do. The actual
//!   `InstancedMesh.setMatrixAt`/`instanceMatrix.needsUpdate` upload is not
//!   ported — that is the future rendering slice's job, consuming
//!   [`GroundShadows::end`]'s placements.
//!
//! ## The animator seam
//!
//! `addActor` reads a foot bone's world position via `agent.animator.bonePos`
//! (`grounding.js:159-162`), and `animator.js` is not ported in this slice.
//! Per the port recipe ("where AI needs something unported... define a
//! narrow trait and say so"), [`FootSource`] names exactly that one call —
//! `agent.animator.bonePos('FootR'/'FootL', out)` — so this module is
//! testable today and a real implementation can bind to the animator once it
//! lands.

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
            let r = u.hypot(v).min(1.0);
            let mut a = (-r * r * power).exp();
            a *= 1.0 - r * r * r; // hard zero at the rim: no visible disc edge
            let i = (y * size + x) * 4;
            buf[i] = 255;
            buf[i + 1] = 255;
            buf[i + 2] = 255;
            buf[i + 3] = (255.0 * a.clamp(0.0, 1.0)).round() as u8;
        }
    }
    buf
}

/// The default texture size/power pair for each sprite. `grounding.js:65-66`.
pub const TEXTURE_SIZE: usize = 64;
pub const BODY_POWER: f64 = 3.4;
pub const FOOT_POWER: f64 = 4.6;

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
