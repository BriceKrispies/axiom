//! **`src/render/pass.js`** — the full-screen triangle infrastructure every
//! post pass in the frame graph hangs off, transcribed.
//!
//! Eighty lines in the source, and the smallest of the three things it defines
//! is the one that matters most: *one* shared geometry, *one* shared scene,
//! *one* shared camera, and a pass is just a material swapped onto the shared
//! mesh. No allocation per frame, and no `examples/jsm` `EffectComposer`.
//!
//! # A triangle, not a quad
//!
//! ```text
//! position: [-1,-1,0,  3,-1,0,  -1,3,0]      (Float32Array)
//! uv:       [ 0, 0,    2, 0,     0, 2   ]    (Float32Array)
//! ```
//!
//! One oversized triangle covering the clip rectangle. A quad would rasterize
//! its two triangles' shared diagonal twice and give the fragment shader a
//! derivative discontinuity along it; a single triangle has neither.
//!
//! # The `uv` attribute is dead
//!
//! `Pass` binds `FS_VERT` from `glsl.js` as **every** pass's vertex shader:
//!
//! ```glsl
//! varying vec2 vUv;
//! void main() {
//!   vUv = position.xy * 0.5 + 0.5;
//!   gl_Position = vec4( position.xy, 0.0, 1.0 );
//! }
//! ```
//!
//! It never reads `uv`. The attribute set in `pass.js` computes, by hand, the
//! same three values `position.xy * 0.5 + 0.5` produces — `(0,0)`, `(2,0)`,
//! `(0,2)` — and is then uploaded and ignored. Dead computation in the source
//! is still part of the source, so [`FULLSCREEN_UVS`] is ported, with a test
//! asserting it is exactly what the vertex stage recomputes.
//!
//! # The one deliberate divergence: `v`
//!
//! WebGL's framebuffer origin is bottom-left and WebGPU's is top-left, so a
//! `vUv` derived identically from clip space addresses the *vertically
//! mirrored* texel on the two APIs. [`FULLSCREEN_WGSL`] therefore emits
//! `vec2(u, 1.0 - v)`. This is renderer convention, not algorithm — the same
//! decision [`crate::cascade`] records for its clip-space `z`, and the same one
//! [`crate::gbuffer::VELOCITY_TEXTURE_V_SIGN`] records for the velocity buffer.
//! A sibling that reproduces `position.xy * 0.5 + 0.5` verbatim gets an
//! upside-down frame.
//!
//! # Storage widths
//!
//! Both attribute arrays are `Float32Array` in the source, so both are `[f32]`
//! here. The bounding sphere is `1e8` at the origin, which is `pass.js`'s way
//! of saying "never frustum-cull me" alongside `frustumCulled = false` — belt
//! and braces in the source, and both ported because a reader will look for
//! both.

/// `new Float32Array([-1, -1, 0, 3, -1, 0, -1, 3, 0])` — three `vec3` clip
/// positions, `z` unused (the vertex stage writes `0.0`).
pub(crate) const FULLSCREEN_POSITIONS: [f32; 9] =
    [-1.0, -1.0, 0.0, 3.0, -1.0, 0.0, -1.0, 3.0, 0.0];

/// `new Float32Array([0, 0, 2, 0, 0, 2])` — three `vec2` UVs that no shader in
/// the frame graph reads. See the module docs.
pub(crate) const FULLSCREEN_UVS: [f32; 6] = [0.0, 0.0, 2.0, 0.0, 0.0, 2.0];

/// `_geometry.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 1e8)`.
pub(crate) const FULLSCREEN_BOUNDING_RADIUS: f32 = 1e8;

/// The vertex count of the shared geometry. Three, and it is not a quad.
pub(crate) const FULLSCREEN_VERTICES: u32 = 3;

/// `FS_VERT` from `src/render/glsl.js`, as WGSL, with the `v` flip the WebGPU
/// framebuffer convention requires (see the module docs).
///
/// Written out rather than assembled from a builtin, on the precedent
/// `surface_program::emit` sets: a WGSL builtin is permitted to factor
/// differently from the GLSL expression it stands in for, and the source's
/// grouping is the specification.
pub(crate) const FULLSCREEN_WGSL: &str = r#"
struct FullscreenVertex {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// The three clip positions of `pass.js`'s oversized triangle, indexed by
// vertex. Written as data rather than derived, because the source writes them
// as data.
const FULLSCREEN_POSITIONS = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 3.0, -1.0),
    vec2<f32>(-1.0,  3.0),
);

@vertex
fn fullscreen_vertex(@builtin(vertex_index) index: u32) -> FullscreenVertex {
    var out: FullscreenVertex;
    let p = FULLSCREEN_POSITIONS[index];
    // `vUv = position.xy * 0.5 + 0.5`, then `v` mirrored: WebGL's framebuffer
    // origin is bottom-left and WebGPU's is top-left.
    let uv = p * 0.5 + 0.5;
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    out.clip_position = vec4<f32>(p, 0.0, 1.0);
    return out;
}
"#;

/// three's blend modes, in three's own numeric order — an enum used as a table
/// index. `pass.js` only ever selects between the first two; `bloom.js`'s
/// upsample selects `Normal`, which is why it is here.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Blending {
    /// `THREE.NoBlending` — the default for every `Pass`.
    None = 0,
    /// `THREE.NormalBlending` — source-alpha over destination.
    Normal = 1,
    /// `THREE.AdditiveBlending`.
    Additive = 2,
}

/// The fixed-function state a `Pass`'s `ShaderMaterial` is built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PassState {
    /// `depthTest: false` — a full-screen triangle is never occluded.
    pub(crate) depth_test: bool,
    /// `depthWrite: false` — and never occludes anything either.
    pub(crate) depth_write: bool,
    /// `opts.blending ?? THREE.NoBlending`.
    pub(crate) blending: Blending,
    /// `opts.blending !== undefined && opts.blending !== THREE.NoBlending`.
    ///
    /// **Not** `blending != None`: passing `NoBlending` *explicitly* yields
    /// `false` here and `NoBlending` above, which is the same pair the default
    /// produces — so the two agree by arithmetic rather than by luck. The
    /// distinction is ported because a reader comparing the files will look for
    /// the `!== undefined` half.
    pub(crate) transparent: bool,
}

/// `new Pass(name, fragmentShader, uniforms, opts)`'s fixed-function state.
///
/// `blending` is the source's `opts.blending`: `None` here is JavaScript's
/// `undefined`, i.e. the caller did not pass one.
pub(crate) fn pass_state(blending: Option<Blending>) -> PassState {
    PassState {
        depth_test: false,
        depth_write: false,
        blending: blending.unwrap_or(Blending::None),
        transparent: blending.is_some_and(|b| b != Blending::None),
    }
}

/// What `blit(renderer, material, target, clear, layer)` does, as a value.
///
/// The whole of it: point the renderer at `target` (a `None` target is the
/// canvas), optionally clear **colour only**, draw the shared mesh. The
/// colour-only clear is the part worth naming — `renderer.clear(true, false,
/// false)` leaves depth and stencil alone, because a full-screen pass writes
/// neither and clearing them would cost a bandwidth pass for nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Blit {
    /// `clear === true` → clear colour before drawing.
    pub(crate) clear_color: bool,
    /// Never cleared by a blit.
    pub(crate) clear_depth: bool,
    /// Never cleared by a blit.
    pub(crate) clear_stencil: bool,
    /// `renderer.setRenderTarget(target, layer)` — the array layer, for the
    /// cascade atlas. Zero for every colour target in the frame graph.
    pub(crate) layer: u32,
}

/// `blit(renderer, material, target, clear = false, layer = 0)`, as a value.
pub(crate) fn blit(clear: bool, layer: u32) -> Blit {
    Blit {
        clear_color: clear,
        clear_depth: false,
        clear_stencil: false,
        layer,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        blit, pass_state, Blending, FULLSCREEN_BOUNDING_RADIUS, FULLSCREEN_POSITIONS,
        FULLSCREEN_UVS, FULLSCREEN_VERTICES, FULLSCREEN_WGSL,
    };

    /// The geometry is one oversized triangle, not two triangles of a quad:
    /// three vertices, and the third clip coordinate reaches `3.0`.
    #[test]
    fn the_shared_geometry_is_one_oversized_triangle() {
        assert_eq!(FULLSCREEN_POSITIONS.len(), (FULLSCREEN_VERTICES * 3) as usize);
        assert_eq!(FULLSCREEN_UVS.len(), (FULLSCREEN_VERTICES * 2) as usize);
        // Two of the three vertices sit outside the clip rectangle, which is
        // what makes one triangle enough to cover it.
        let outside = FULLSCREEN_POSITIONS
            .chunks_exact(3)
            .filter(|v| (v[0] > 1.0) | (v[1] > 1.0))
            .count();
        assert_eq!(outside, 2, "an oversized triangle has two vertices past the edge");
        // Every z is zero: the vertex stage writes its own.
        assert!(FULLSCREEN_POSITIONS.chunks_exact(3).all(|v| v[2] == 0.0));
        assert_eq!(FULLSCREEN_BOUNDING_RADIUS, 1e8);
    }

    /// The dead `uv` attribute is bit-for-bit what `FS_VERT` recomputes from
    /// `position.xy * 0.5 + 0.5`. That equality is the *evidence* it is dead:
    /// if the two ever disagreed, one of them would be observable.
    #[test]
    fn the_uploaded_uvs_duplicate_what_the_vertex_stage_recomputes() {
        let recomputed: Vec<f32> = FULLSCREEN_POSITIONS
            .chunks_exact(3)
            .flat_map(|v| [v[0] * 0.5 + 0.5, v[1] * 0.5 + 0.5])
            .collect();
        assert_eq!(recomputed, FULLSCREEN_UVS.to_vec());
        // ...and the WGSL that replaces FS_VERT carries the same three
        // positions and the flip that WebGPU's framebuffer origin requires.
        assert!(FULLSCREEN_WGSL.contains("vec2<f32>( 3.0, -1.0)"));
        assert!(FULLSCREEN_WGSL.contains("vec2<f32>(-1.0,  3.0)"));
        assert!(FULLSCREEN_WGSL.contains("p * 0.5 + 0.5"));
        assert!(
            FULLSCREEN_WGSL.contains("1.0 - uv.y"),
            "the v flip is the one deliberate divergence and must be visible in the text"
        );
    }

    /// `transparent` keys off whether a blend mode was *supplied*, not off what
    /// it is — so an explicit `NoBlending` and an omitted one produce the same
    /// state through different arithmetic.
    #[test]
    fn transparency_follows_whether_a_blend_mode_was_supplied() {
        let default = pass_state(None);
        assert_eq!(default.blending, Blending::None);
        assert!(!default.transparent);

        let explicit_none = pass_state(Some(Blending::None));
        assert_eq!(explicit_none, default, "an explicit NoBlending is the default");

        let normal = pass_state(Some(Blending::Normal));
        assert_eq!(normal.blending, Blending::Normal);
        assert!(normal.transparent);

        let additive = pass_state(Some(Blending::Additive));
        assert!(additive.transparent);

        // Depth is off in every case: a full-screen triangle neither tests nor
        // writes it.
        assert!([default, explicit_none, normal, additive]
            .iter()
            .all(|s| !s.depth_test & !s.depth_write));
    }

    /// three's blend constants are a numeric enum and this one is a table
    /// index; the order is pinned so a later insertion cannot renumber it.
    #[test]
    fn the_blend_modes_keep_threes_numbering() {
        assert_eq!(Blending::None as u8, 0);
        assert_eq!(Blending::Normal as u8, 1);
        assert_eq!(Blending::Additive as u8, 2);
    }

    /// A blit clears colour or nothing, and never depth or stencil.
    #[test]
    fn a_blit_clears_colour_only_and_never_depth() {
        let plain = blit(false, 0);
        assert!(!plain.clear_color);
        let cleared = blit(true, 0);
        assert!(cleared.clear_color);
        assert!([plain, cleared]
            .iter()
            .all(|b| !b.clear_depth & !b.clear_stencil));
        // The layer argument exists for the cascade array target.
        assert_eq!(blit(false, 3).layer, 3);
    }
}
