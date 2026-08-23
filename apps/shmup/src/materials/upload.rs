//! Ported from Claude-of-Duty `src/materials/generator.js:260-321` — the
//! **write side** of the bake: the render-target write that
//! [`super::bake`] deliberately stopped short of.
//!
//! [`super::bake::bake`] produces `f32` channels in `[0, 1]` and says so in its
//! own doc: "nothing in this port ever quantizes to 8-bit, since there is no
//! display path yet that would need it to." This module is that display path.
//! In the source the quantization is the hardware's — `TextureForge.build`
//! renders each pass into a `THREE.WebGLRenderTarget` with the default
//! `UnsignedByteType`, so every channel lands as one round-to-nearest byte
//! (`generator.js:174-198` for the scratch target, `260-321` for the three real
//! ones). Here it is [`quantize`], and it happens exactly once, at the boundary
//! where pixels leave the port and become engine upload data.
//!
//! ## What the engine binds, and how the source's slots map onto it
//!
//! The GPU backend binds, per material:
//!
//! ```text
//! @group(0) 0 albedo_tex   1 albedo_sampler
//!           2 normal_tex   3 normal_sampler
//!           4 material_orm_tex     (occlusion, roughness, metalness, height)
//!           5 material_detail_tex  (normal.x, normal.y, micro_albedo, height)
//!           6 material_macro_tex   (the four variation bands)
//! ```
//!
//! against the source's (`index.js:206-210`, `shader.js:811-813`):
//!
//! ```text
//! mat.map          = set.albedo    -> albedo_tex          (sRGB rgb, height a)
//! mat.normalMap    = set.normal    -> normal_tex          (linear)
//! mat.roughnessMap = set.orm       -> material_orm_tex    (linear, +height in a)
//! owDetailNrm      = detail.normal \
//! owDetailTex      = detail.albedo / -> material_detail_tex
//! owMacroTex       = macro.albedo  -> material_macro_tex
//! ```
//!
//! Two of those mappings are not one-to-one and are worth stating plainly:
//!
//! * **ORM carries the height too.** The source's ORM target has `a = 1`
//!   ([`super::bake::bake`] writes it), and Three.js reads height out of
//!   `map.a`. Axiom's binding 4 is documented `(occlusion, roughness,
//!   metalness, height)`, so [`orm_height_map`] moves the albedo's alpha into
//!   the ORM alpha. Nothing is invented — it is the same height field, put
//!   where this engine's shader looks for it.
//! * **The detail lane is two source textures and one engine binding.** The
//!   source samples the shared detail maps through two texture units,
//!   `owDetailNrm` and `owDetailTex`. Between them it consumes exactly **four**
//!   scalars: `detailNormal.xy` (both consumers are UDN — they sum the tangent
//!   xy and keep the base z, so `detailNormal.z` is never read),
//!   `detailAlbedo.r` (the micro albedo/roughness speckle) and
//!   `detailAlbedo.a` (the micro height). Four scalars fit four channels, so
//!   binding 5 packs `(normal.x, normal.y, micro_albedo, height)` and the
//!   two-unit lane is carried **losslessly** in one map.
//!
//!   This replaced an earlier `(normal.rgb, height.a)` packing that had nowhere
//!   to put `detailAlbedo.r`, so the shader's `(dTex.r - 0.5) * 1.25` term read
//!   the normal's *x* instead. `material_shader::compose` unpacks the logical
//!   `owDetailTex` texel from `.b`/`.a` at the sample site, which keeps
//!   `material_shader::detail` the faithful two-texture definition of the
//!   source and confines the packing to the composition that chose it.
//!
//! ## Colour space
//!
//! `albedo` is uploaded as `Rgba8UnormSrgb`, and [`super::bake::bake`] already
//! sRGB-encoded it (`linear_albedo: false`) exactly as the source's
//! `SRGBColorSpace` render target did — so the two encodes agree and the GPU
//! decode returns the linear colour the surface function computed. Every other
//! map is **linear data** and is uploaded `Rgba8Unorm`: the source's own
//! `linearAlbedo: true` on `buildDetail`/`buildMacro` exists for precisely this
//! reason ("the detail map is DATA, not colour" — `generator.js:355-358`), and
//! `bake` honours it, so nothing here re-encodes them.

use crate::config::Quality;
use crate::materials::bake::{BakedSet, Texture};
use crate::materials::system::{MaterialOpts, MaterialSystem, RendererCaps};

/// One RGBA8 map, row-major, `width * height * 4` bytes — the shape
/// `RunningApp::add_texture_data` takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba8Map {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba8Map {
    /// The RGBA8 texel at `(x, y)`.
    pub fn texel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }
}

/// One surface's three per-material maps, in the engine's binding order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceMaps {
    /// Binding 0. sRGB-encoded colour in RGB, height in A.
    pub albedo: Rgba8Map,
    /// Binding 2. Tangent-space normal, OpenGL +Y, `* 0.5 + 0.5`. Linear.
    pub normal: Rgba8Map,
    /// Binding 4. `(occlusion, roughness, metalness, height)`. Linear.
    pub orm_height: Rgba8Map,
}

/// Everything the street needs uploaded: one [`SurfaceMaps`] per baked surface,
/// keyed by the **bake key** (so two palette entries that differ only by a tint
/// share one entry, which is the whole point of `bakeKey` —
/// `system.rs`'s module doc), plus the two shared maps every material samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakedLibrary {
    /// `(bake key, maps)` in bake order — the order the source's lazy
    /// `get(name)` walk would have produced them in.
    pub surfaces: Vec<(String, SurfaceMaps)>,
    /// Binding 5, shared by every material. `(normal.rgb, height.a)`.
    pub detail: Rgba8Map,
    /// Binding 6, shared by every material. The four variation bands.
    pub macro_field: Rgba8Map,
    /// Library **name** to bake **key**, in the order `names` was given.
    ///
    /// `surfaces` is keyed by bake key and deduplicated, because that is what a
    /// bake costs — but a caller holds palette names, and forty-six of them
    /// collapse onto nineteen keys. Without this the caller has to rebuild a
    /// `MaterialSystem` purely to re-derive a mapping this function already
    /// computed and threw away.
    pub names: Vec<(String, String)>,
}

impl BakedLibrary {
    /// The maps for one bake key, or `None` if it was not baked.
    pub fn get(&self, bake_key: &str) -> Option<&SurfaceMaps> {
        self.surfaces
            .iter()
            .find(|(key, _)| key == bake_key)
            .map(|(_, maps)| maps)
    }

    /// Total bytes across every map — what the upload actually costs.
    pub fn bytes(&self) -> usize {
        self.surfaces
            .iter()
            .map(|(_, m)| m.albedo.pixels.len() + m.normal.pixels.len() + m.orm_height.pixels.len())
            .sum::<usize>()
            + self.detail.pixels.len()
            + self.macro_field.pixels.len()
    }
}

/// A `[0, 1]` float channel as one byte, the way a `UnsignedByteType` render
/// target writes it: clamp, scale by 255, round half away from zero.
///
/// `+ 0.5` then truncate is round-half-up, which for a non-negative value is
/// the same as `round`. The clamp comes first because a surface function may
/// return a value fractionally outside `[0, 1]` (the source's own
/// `owSurface` bodies are not all clamped) and a wrapped `as u8` would turn an
/// over-bright texel black.
pub fn quantize(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// A baked [`Texture`]'s four channels, straight through [`quantize`].
fn map_of(texture: &Texture) -> Rgba8Map {
    let size = texture.size;
    let mut pixels = Vec::with_capacity((size as usize) * (size as usize) * 4);
    for y in 0..size {
        for x in 0..size {
            let texel = texture.get(x, y);
            pixels.extend_from_slice(&[
                quantize(texel[0]),
                quantize(texel[1]),
                quantize(texel[2]),
                quantize(texel[3]),
            ]);
        }
    }
    Rgba8Map {
        width: size,
        height: size,
        pixels,
    }
}

/// Binding 4: the ORM triple with the **albedo's** alpha (the height field)
/// carried into the alpha channel. See the module doc.
fn orm_height_map(orm: &Texture, albedo: &Texture) -> Rgba8Map {
    let size = orm.size;
    let mut pixels = Vec::with_capacity((size as usize) * (size as usize) * 4);
    for y in 0..size {
        for x in 0..size {
            let o = orm.get(x, y);
            let height = albedo.get(x, y)[3];
            pixels.extend_from_slice(&[
                quantize(o[0]),
                quantize(o[1]),
                quantize(o[2]),
                quantize(height),
            ]);
        }
    }
    Rgba8Map {
        width: size,
        height: size,
        pixels,
    }
}

/// Binding 5: the shared micro-detail tile, packed
/// `(normal.x, normal.y, micro_albedo, height)`.
///
/// The normal's **z is dropped, not lost**: both consumers of `dn` are UDN and
/// read only its xy, and `material_shader::compose` reconstructs
/// `z = sqrt(max(0, 1 - dot(xy, xy)))` at the sample site. That frees `.b` for
/// `detailAlbedo.r`, the micro-albedo speckle, which the previous
/// `(normal.rgb, height.a)` packing had no channel for.
fn detail_map(detail: &BakedSet) -> Rgba8Map {
    let normal = detail
        .normal
        .as_ref()
        .expect("buildDetail bakes a normal (generator.js:345-363)");
    let size = normal.size;
    let mut pixels = Vec::with_capacity((size as usize) * (size as usize) * 4);
    for y in 0..size {
        for x in 0..size {
            let n = normal.get(x, y);
            let albedo = detail.albedo.get(x, y);
            pixels.extend_from_slice(&[
                quantize(n[0]),
                quantize(n[1]),
                quantize(albedo[0]),
                quantize(albedo[3]),
            ]);
        }
    }
    Rgba8Map {
        width: size,
        height: size,
        pixels,
    }
}

/// One surface's three maps from its baked set.
///
/// # Panics
///
/// If the set was baked without ORM or without a normal. Every per-material
/// bake asks for both (`TextureSet::bake` passes `want_orm: true,
/// want_normal: true`, which is `index.js` passing neither flag), so this is a
/// contract, not a runtime condition.
pub fn surface_maps(set: &BakedSet) -> SurfaceMaps {
    let orm = set
        .orm
        .as_ref()
        .expect("a per-material bake asks for ORM (index.js passes no `orm` flag)");
    let normal = set
        .normal
        .as_ref()
        .expect("a per-material bake asks for a normal (index.js passes no `normal` flag)");
    SurfaceMaps {
        albedo: map_of(&set.albedo),
        normal: map_of(normal),
        orm_height: orm_height_map(orm, &set.albedo),
    }
}

/// Bake `names` through the real [`MaterialSystem`] cache and quantize the
/// result — the whole street's texture library, in one call.
///
/// `names` are **library names or aliases**; the system resolves each one and
/// collapses duplicates onto one bake key, so passing a palette's forty-six
/// entries produces the nineteen bakes the source produces. Names are baked in
/// the order given, and a name already covered by an earlier bake key is
/// skipped without re-baking, exactly as `_sets` does.
///
/// `quality` scales every size through `MaterialSystem::size_of` — the source's
/// own texture budget knob. `size_cap` clamps the result on top of it;
/// `u32::MAX` means "whatever the library authored", which is the faithful
/// bake and, at 1024², **minutes** of CPU work (see [`RUNTIME_BAKE_SIZE`]).
/// The anisotropy value is the engine's own concern (`TextureSampling`), so it
/// is passed as the source's default 8 and read by nothing here.
pub fn bake_library(quality: Quality, size_cap: u32, names: &[&str]) -> BakedLibrary {
    let mut system = MaterialSystem::new(Some(RendererCaps {
        max_anisotropy: Some(8.0),
    }));
    system.configure(quality, 8);
    let opts = MaterialOpts::new();

    let mut surfaces: Vec<(String, SurfaceMaps)> = Vec::new();
    let mut resolved: Vec<(String, String)> = Vec::new();
    for name in names {
        let key = system.texture_set_key(name, &opts);
        let Some(key) = key else { continue };
        resolved.push(((*name).to_string(), key.clone()));
        if surfaces.iter().any(|(existing, _)| *existing == key) {
            continue;
        }
        let Some(set) = system.texture_set(&key) else {
            continue;
        };
        let size = set.size.min(size_cap);
        surfaces.push((key, surface_maps(&set.bake_at(size, true, true))));
    }

    let shared = system
        .shared()
        .expect("configure with a renderer builds the shared maps (index.js:68-93)");
    let detail = super::bake::build_detail(shared.detail_size.min(size_cap), 1.0);
    let macro_set = super::bake::build_macro(shared.macro_size.min(size_cap), 2.0);
    BakedLibrary {
        surfaces,
        detail: detail_map(&detail),
        macro_field: map_of(&macro_set.albedo),
        names: resolved,
    }
}

// ---------------------------------------------------------------------------
// The affordable subset: albedo only, size-capped.
// ---------------------------------------------------------------------------

/// The size cap the app's boot-time bake runs at, and the **only** number in
/// this port that is not the source's.
///
/// The source bakes at 1024² (512² for six of the nineteen) on the GPU: four
/// full-screen draws per surface, all nineteen in ~1.3 s. This port bakes on
/// the CPU, and the cost is not close. Measured, native, `--release`, on this
/// machine:
///
/// ```text
/// one 512² surface (asphalt)   16.6 s
/// all nineteen at 512²        232   s
/// all nineteen at 1024²      ~930   s   (quadratic in size)
/// ```
///
/// ~15.5 µs per `owSurface` evaluation, and the library's own resolutions need
/// 57 million of them. The cause is structural, not a missing optimisation:
/// `ow_hash22` is the classic `fract(sin(dot(…)))` GLSL hash, which is one
/// instruction on a GPU and a `f64::sin` on a CPU, and a single surface
/// evaluation makes hundreds of them. No CPU rewrite closes a 100x gap that is
/// really 1024²-way parallelism.
///
/// So the boot-time bake runs **albedo only** (one evaluation per texel instead
/// of three) at this cap. 64² over a 2 m tile is 3 cm per texel: coarse, but it
/// is the real generator's colour field, and the material shader's macro,
/// weathering and cavity layers — which are per-pixel and cost nothing here —
/// carry the high frequencies on top of it.
///
/// **The fix is the source's own**: bake on the GPU. `bake.rs`'s module doc
/// already spells out what that needs (WGSL emission of `owSurface`, a
/// half-float scratch height target, the Sobel as a fragment shader), and
/// `sobel` is written to be line-for-line portable to it. Until then this
/// constant is the honest ceiling, and raising it costs boot time quadratically.
pub const RUNTIME_BAKE_SIZE: u32 = 64;

/// Bake the **albedo** map for each of `names`, capped at `size_cap` — the only
/// part of the bake the app can afford at boot *and* the only part the engine
/// can bind today (`axiom_host`'s material contract carries albedo pixels and
/// nothing else; see the notes file for the extension the other three maps
/// need).
///
/// Returns one entry per *input name*, in input order, so a caller holding a
/// palette key can look its library name up directly. Names that resolve to the
/// same bake key share one bake — the collapse `bakeKey` exists for — so the
/// nineteen-name street costs nineteen bakes and the forty-six-entry palette
/// costs the same nineteen.
///
/// The albedo's alpha is the height field, exactly as in the full bake. Nothing
/// samples it yet (parallax needs binding 4), but dropping it would make this
/// map differ from [`bake_library`]'s for no reason.
pub fn bake_albedo_maps(names: &[&str], size_cap: u32) -> Vec<(String, Rgba8Map)> {
    let mut system = MaterialSystem::new(Some(RendererCaps {
        max_anisotropy: Some(8.0),
    }));
    system.configure(Quality::Ultra, 8);
    let opts = MaterialOpts::new();

    let mut baked: Vec<(String, Rgba8Map)> = Vec::new();
    let mut out: Vec<(String, Rgba8Map)> = Vec::new();
    for name in names {
        let Some(key) = system.texture_set_key(name, &opts) else {
            continue;
        };
        let existing = baked
            .iter()
            .find(|(baked_key, _)| *baked_key == key)
            .map(|(_, map)| map.clone());
        let map = existing.unwrap_or_else(|| {
            let set = system
                .texture_set(&key)
                .expect("texture_set_key just inserted this key");
            let size = set.size.min(size_cap);
            map_of(&set.bake_at(size, false, false).albedo)
        });
        baked.push((key, map.clone()));
        out.push(((*name).to_string(), map));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-to-nearest at the two ends and at the exact half, which is where a
    /// truncating `as u8` and a rounding one disagree.
    #[test]
    fn quantize_rounds_to_nearest_and_clamps() {
        assert_eq!(quantize(0.0), 0);
        assert_eq!(quantize(1.0), 255);
        assert_eq!(quantize(0.5), 128);
        // 127.5 -> 128, not 127.
        assert_eq!(quantize(127.5 / 255.0), 128);
        // Out of range in both directions saturates rather than wrapping.
        assert_eq!(quantize(-3.0), 0);
        assert_eq!(quantize(4.0), 255);
        assert_eq!(quantize(f32::NAN), 0);
    }

    /// A one-name bake produces every map at the library's own size, and the
    /// shared maps at the sizes `_tryBuild` asks for.
    #[test]
    fn a_small_bake_produces_every_map() {
        let library = bake_library(Quality::Low, 16, &["glass"]);
        assert_eq!(library.surfaces.len(), 1);
        let maps = &library.surfaces[0].1;
        assert_eq!(maps.albedo.width, 16);
        assert_eq!(maps.albedo.width, maps.albedo.height);
        assert_eq!(
            maps.albedo.pixels.len(),
            (maps.albedo.width as usize) * (maps.albedo.height as usize) * 4
        );
        assert_eq!(maps.normal.width, maps.albedo.width);
        assert_eq!(maps.orm_height.width, maps.albedo.width);
        // The ORM's alpha is the albedo's alpha — the height field.
        assert_eq!(maps.orm_height.texel(0, 0)[3], maps.albedo.texel(0, 0)[3]);
        assert_eq!(library.macro_field.width, 16);
        assert_eq!(library.detail.width, 16);
        assert!(library.bytes() > 0);
    }

    /// Two names that resolve to one bake key bake once. This is the collapse
    /// the whole cache exists for.
    #[test]
    fn an_alias_does_not_bake_twice() {
        let library = bake_library(Quality::Low, 8, &["glass", "glass"]);
        assert_eq!(library.surfaces.len(), 1);
        assert!(library.get(&library.surfaces[0].0).is_some());
        assert!(library.get("no-such-bake").is_none());
    }

    /// An unknown name resolves to concrete rather than vanishing, so it still
    /// bakes — the source's `_resolve` fallback.
    #[test]
    fn an_unknown_name_still_bakes_its_fallback() {
        let library = bake_library(Quality::Low, 8, &["no-such-surface"]);
        assert_eq!(library.surfaces.len(), 1);
        assert!(library.surfaces[0].0.starts_with("concrete|"));
    }

    /// The runtime path returns one map per *input* name (so a palette key can
    /// look its library name up directly) while baking each distinct bake key
    /// once — and it caps the size.
    #[test]
    fn the_runtime_albedo_bake_is_capped_and_deduped() {
        let maps = bake_albedo_maps(&["glass", "window", "rubber"], 16);
        assert_eq!(maps.len(), 3);
        assert_eq!(maps[0].0, "glass");
        // `window` is an alias of `glass`, so its map is the same texels.
        assert_eq!(maps[1].0, "window");
        assert_eq!(maps[0].1, maps[1].1);
        assert_ne!(maps[0].1, maps[2].1);
        maps.iter().for_each(|(_, m)| {
            assert_eq!(m.width, 16);
            assert_eq!(m.height, 16);
            assert_eq!(m.pixels.len(), 16 * 16 * 4);
        });
    }

    /// The cap is a **cap**, not a size: a surface authored smaller than it
    /// keeps its own resolution. `foliage` is authored at 512, the smallest
    /// size the library uses, and is the cheapest surface to prove it on.
    #[test]
    fn the_cap_never_upsamples_a_smaller_surface() {
        let maps = bake_albedo_maps(&["foliage"], 4096);
        assert_eq!(maps[0].1.width, 512);
    }
}
