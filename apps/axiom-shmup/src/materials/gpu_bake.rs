//! The GPU bake's **app half**: what to bake, with which uniforms, in which
//! order — and how to repack what comes back into the maps the engine binds.
//!
//! Ported from Claude-of-Duty `src/materials/index.js:68-154`
//! (`MaterialSystem._tryBuild` and `getTextureSet`), which is the code that
//! *drives* `TextureForge.build`. The forge itself is engine machinery and
//! lives in `axiom_gpu_backend`'s `texture_bake`; the nineteen `owSurface`
//! bodies are in [`crate::materials::wgsl`]. This module is the third piece:
//! the bake list.
//!
//! ## Why the GPU, and what it replaces
//!
//! [`crate::materials::upload::bake_library`] is the same list on the CPU, and
//! it is correct — it is also 16.6 s for one 512² surface and ~930 s for the
//! nineteen at their authored sizes, against ~1.3 s for all of them on the GPU
//! in the source. That is why the app ships
//! [`crate::materials::upload::RUNTIME_BAKE_SIZE`] = 64 today, and why the
//! street's walls show a 64 px tile stretched over a building. This module is
//! the path off that.
//!
//! One correction to that constant's doc while its measurements are being
//! quoted: it attributes the cost to `ow_hash22` being "the classic
//! `fract(sin(dot(…)))` GLSL hash". It is not — `noise.js:11` says *"hashes are
//! sin-free (Dave Hoskins style)"*, and `ow_hash22` is pure `fract`/multiply
//! churn. The transcendentals are in `owGrad2` (a `cos` and a `sin` per lattice
//! corner, four corners per `owNoise`, four octaves per fbm), so a four-octave
//! fbm is 32 of them. The *measurements* stand and so does the conclusion; only
//! the attribution is wrong.
//!
//! ## The boundary, named
//!
//! `axiom_host::ProceduralBakeRequest` / `ProceduralBakeMaps` — the host
//! layer's backend-neutral bake contract. This module builds requests and
//! repacks answers; it never names a backend, and it never touches a device.
//! A caller that has one (the parity test today; the engine's install path once
//! the lane below exists) runs the plan. That keeps the whole bake list natively
//! testable with no GPU, and it is the only tier allowed to join the two
//! contracts — apps are where module contracts are translated.
//!
//! ## What is still not wired, and what would make it live
//!
//! The *binding* side is done and this is worth stating plainly, because the
//! deferral recorded in `notes/materials-upload.md` has **expired**:
//! `axiom_host::MaterialTexture` now carries all four non-albedo maps
//! (`with_normal`, `with_orm_height`, `with_detail`, `with_macro_field`) and
//! `axiom::Material` carries the four ids that resolve them. Every map this
//! module produces is bindable today, through `RunningApp::add_texture_data`
//! and `Material::with_*_texture`, with no engine change at all.
//!
//! What is missing is only the *execution* lane: nothing can yet be handed a
//! bake to run at level load. It needs
//! `modules/axiom-gpu-backend/src/gpu_backend_api/mod.rs` to publish the bake
//! entry (offscreen, done in this slice), then `modules/axiom/src/app/` and
//! `modules/axiom-windowing` to carry a request down to
//! `live_gpu_binding.rs`, which is where the bake must actually run for the
//! browser.
//!
//! **The expiry check.** `crate::scene::app`'s install still calls
//! `upload::bake_albedo_maps` — albedo only, 64², CPU. When the lane above
//! lands, that call must become this module's plan, and `scene::app` must set
//! the four extra texture ids on each `Material`. If it does not, this file is
//! a deferral that quietly stopped being one, which is the failure mode this
//! port has already hit four times.

use crate::config::Quality;
use crate::materials::surfaces::metal::hex_to_linear_tint;
use crate::materials::system::{MaterialOpts, MaterialSystem, RendererCaps, TextureSet};
use crate::materials::upload::{BakedLibrary, Rgba8Map, SurfaceMaps};
use crate::materials::wgsl;
use axiom_host::{ProceduralBakeMaps, ProceduralBakeRequest};

/// Every bake the street needs, **in the order the source performs them**:
/// the two shared maps first (`index.js:82-83`, inside `_tryBuild`, before any
/// material exists), then one per distinct bake key in the order the names were
/// asked for.
///
/// That order is not cosmetic — it is what `tests/materials_upload/golden.json`
/// pins, and it is the order a bake-count or a timing log would report.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuBakePlan {
    /// `buildDetail(this._size(1024))` — `generator.js:344-363`.
    pub detail: ProceduralBakeRequest,
    /// `buildMacro(256)` — `generator.js:365-381`.
    pub macro_field: ProceduralBakeRequest,
    /// One per distinct bake key, in the order `names` asked for them.
    pub surfaces: Vec<ProceduralBakeRequest>,
}

impl GpuBakePlan {
    /// Every request, in bake order — shared maps first. This is the slice a
    /// caller with a device iterates.
    pub fn in_bake_order(&self) -> Vec<&ProceduralBakeRequest> {
        core::iter::once(&self.detail)
            .chain(core::iter::once(&self.macro_field))
            .chain(self.surfaces.iter())
            .collect()
    }

    /// How many `TextureForge.build` calls this plan is —
    /// `MaterialSystem::bake_count` plus the two shared maps.
    pub fn len(&self) -> usize {
        self.surfaces.len() + 2
    }

    /// Never — a plan always carries the two shared maps. Present because
    /// clippy asks for it beside `len`, and because "is the plan empty" is a
    /// question with a definite answer rather than an omitted one.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Total full-screen draws this plan costs, summed over every request. The
    /// source's own accounting: "a full 1K set costs one framebuffer bind and
    /// four full-screen draws" (`generator.js:12`).
    pub fn draw_count(&self) -> u32 {
        self.in_bake_order()
            .iter()
            .map(|request| request.pass_count())
            .sum()
    }
}

/// `def.tintA !== undefined ? new THREE.Color(bake.tintA) : undefined`
/// (`index.js:145`), with the forge's own `uTintA` default of white when it is
/// absent (`generator.js:228`, `271` — the uniform keeps its initial value when
/// `def.tintA` is falsy).
fn tint(hex: Option<u32>) -> [f32; 3] {
    hex.map_or([1.0, 1.0, 1.0], |value| {
        let linear = hex_to_linear_tint(value);
        [linear.x as f32, linear.y as f32, linear.z as f32]
    })
}

/// `new THREE.Vector4().fromArray(bake.param)` (`index.js:147`): a shorter
/// array leaves the remaining lanes at zero, and an absent `param` is all
/// zeroes (`generator.js:230`).
fn param(values: Option<&Vec<f64>>) -> [f32; 4] {
    let read = |lane: usize| {
        values
            .and_then(|list| list.get(lane))
            .copied()
            .unwrap_or(0.0) as f32
    };
    [read(0), read(1), read(2), read(3)]
}

/// One resolved [`TextureSet`] as a bake request, capped at `size_cap`.
///
/// `size_cap` is not the source's — see
/// [`crate::materials::upload::RUNTIME_BAKE_SIZE`]. On the GPU it exists only
/// so a test can bake nineteen surfaces without waiting on the *CPU* reference
/// it is compared against; a real bake passes `u32::MAX`.
///
/// `linear_albedo: false`, `want_orm: true`, `want_normal: true` are the
/// request type's own defaults, and they are the right ones here because
/// `index.js` passes none of the three flags, so `generator.js`'s `!== false` /
/// `!== true` defaults all apply.
fn surface_request(set: &TextureSet, key: &str, size_cap: u32) -> ProceduralBakeRequest {
    ProceduralBakeRequest::new(
        key.to_string(),
        wgsl::generator_wgsl(set.generator).to_string(),
        set.size.min(size_cap),
    )
    .with_seed(set.seed as f32)
    .with_tints(tint(set.tint_a), tint(set.tint_b))
    .with_param(param(set.param.as_ref()))
    .with_scale(set.world_size as f32, set.relief as f32)
}

/// `buildDetail(size)` (`generator.js:344-363`) as a request.
fn detail_request(size: u32) -> ProceduralBakeRequest {
    ProceduralBakeRequest::new("__detail".to_string(), wgsl::noise::DETAIL.to_string(), size)
        .with_seed(1.0)
        // 1.6 mm grain standing ~0.4 mm proud: a real tooth, not a bump-map
        // hint.
        .with_scale(0.25, 0.0034)
        // "The detail map is DATA, not colour" (generator.js:355-358).
        .with_linear_albedo(true)
        // "Only the albedo … and the derived normal are sampled; the ORM output
        // was never bound anywhere" (generator.js:359-361).
        .with_maps(false, true)
}

/// `buildMacro(size)` (`generator.js:365-381`) as a request.
fn macro_request(size: u32) -> ProceduralBakeRequest {
    ProceduralBakeRequest::new("__macro".to_string(), wgsl::noise::MACRO.to_string(), size)
        .with_seed(2.0)
        .with_scale(32.0, 0.5)
        // "Macro is data, not colour — it must be stored and sampled linearly"
        // (generator.js:367).
        .with_linear_albedo(true)
        // "Four bands packed into the albedo output is the whole map; the macro
        // ORM and macro normal were baked and then never sampled."
        .with_maps(false, false)
}

/// The whole street's bake list.
///
/// `names` are library names or aliases; each resolves through the system's
/// alias table and collapses onto one bake key, so a forty-six-entry palette
/// produces the same nineteen bakes the source produces. This is the exact walk
/// [`crate::materials::upload::bake_library`] performs — the two must plan the
/// same bakes, which is what makes a GPU/CPU comparison meaningful.
pub fn plan(quality: Quality, size_cap: u32, names: &[&str]) -> GpuBakePlan {
    let mut system = MaterialSystem::new(Some(RendererCaps {
        max_anisotropy: Some(8.0),
    }));
    system.configure(quality, 8);
    let opts = MaterialOpts::new();

    let mut surfaces: Vec<ProceduralBakeRequest> = Vec::new();
    for name in names {
        let Some(key) = system.texture_set_key(name, &opts) else {
            continue;
        };
        if surfaces.iter().any(|request| request.key() == key) {
            continue;
        }
        let set = system
            .texture_set(&key)
            .expect("texture_set_key just inserted this key");
        surfaces.push(surface_request(set, &key, size_cap));
    }

    let shared = system
        .shared()
        .expect("configure with a renderer builds the shared maps (index.js:68-93)");
    GpuBakePlan {
        detail: detail_request(shared.detail_size.min(size_cap)),
        macro_field: macro_request(shared.macro_size.min(size_cap)),
        surfaces,
    }
}

// ---------------------------------------------------------------------------
// Repacking. These move whole bytes between channels; they never quantize,
// because the GPU already did that on write to an 8-bit target. That is the one
// structural difference from `upload`'s `orm_height_map` / `detail_map`, which
// take `f32` textures and quantize as they pack.
// ---------------------------------------------------------------------------

fn rgba8(size: u32, pixels: Vec<u8>) -> Rgba8Map {
    Rgba8Map {
        width: size,
        height: size,
        pixels,
    }
}

/// `(rgb.rgb, alpha.a)` — one map's colour with another's alpha.
fn with_alpha_of(size: u32, rgb: &[u8], alpha: &[u8]) -> Rgba8Map {
    let pixels = (0..(size as usize) * (size as usize))
        .flat_map(|texel| {
            let at = texel * 4;
            [rgb[at], rgb[at + 1], rgb[at + 2], alpha[at + 3]]
        })
        .collect();
    rgba8(size, pixels)
}

/// One surface's three engine-facing maps.
///
/// Binding 4 is documented `(occlusion, roughness, metalness, height)` while
/// the source's ORM target has `a = 1` and Three.js reads height out of
/// `map.a`, so the height moves from the albedo's alpha into the ORM's —
/// exactly what `upload::orm_height_map` does, on bytes instead of floats.
///
/// # Panics
///
/// If the maps were baked without ORM or without a normal. Every per-material
/// request asks for both ([`surface_request`]), so this is a contract.
pub fn surface_maps(maps: &ProceduralBakeMaps) -> SurfaceMaps {
    let orm = maps
        .orm()
        .expect("a per-material bake asks for ORM (index.js passes no `orm` flag)");
    let normal = maps
        .normal()
        .expect("a per-material bake asks for a normal (index.js passes no `normal` flag)");
    SurfaceMaps {
        albedo: rgba8(maps.size(), maps.albedo().to_vec()),
        normal: rgba8(maps.size(), normal.to_vec()),
        orm_height: with_alpha_of(maps.size(), orm, maps.albedo()),
    }
}

/// Binding 5: the shared micro-detail tile, `(normal.rgb, height.a)`.
///
/// The packing the backend currently documents. It leaves the source's
/// `detailAlbedo.r` — the micro albedo/roughness the shader's
/// `(dTex.r - 0.5) * 1.25` term reads — with nowhere to go; that is an engine
/// contract gap recorded in `notes/materials-upload.md`, not something to paper
/// over by writing into a channel the shader does not read.
///
/// # Panics
///
/// If the detail bake produced no normal. [`detail_request`] always asks for
/// one.
pub fn detail_map(maps: &ProceduralBakeMaps) -> Rgba8Map {
    let normal = maps
        .normal()
        .expect("buildDetail bakes a normal (generator.js:345-363)");
    with_alpha_of(maps.size(), normal, maps.albedo())
}

/// The plan plus the device's answers, as the [`BakedLibrary`] the upload path
/// already knows how to consume.
///
/// `surfaces` must be the answers to [`GpuBakePlan::surfaces`], in order.
///
/// # Panics
///
/// If the answer count does not match the plan — a caller that dropped a bake
/// would otherwise silently pair each key with the wrong texels.
pub fn assemble(
    plan: &GpuBakePlan,
    detail: &ProceduralBakeMaps,
    macro_field: &ProceduralBakeMaps,
    surfaces: &[ProceduralBakeMaps],
) -> BakedLibrary {
    assert_eq!(
        surfaces.len(),
        plan.surfaces.len(),
        "the plan asked for {} surface bakes and got {}",
        plan.surfaces.len(),
        surfaces.len()
    );
    BakedLibrary {
        surfaces: plan
            .surfaces
            .iter()
            .zip(surfaces.iter())
            .map(|(request, maps)| (request.key().to_string(), surface_maps(maps)))
            .collect(),
        detail: detail_map(detail),
        macro_field: rgba8(macro_field.size(), macro_field.albedo().to_vec()),
        // The GPU plan names its bakes by key already, so name and key are the
        // same string here; the CPU path is the one where forty-six names
        // collapse onto nineteen keys.
        names: plan
            .surfaces
            .iter()
            .map(|request| (request.key().to_string(), request.key().to_string()))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maps(size: u32, fill: u8, orm: bool, normal: bool) -> ProceduralBakeMaps {
        let channel = |base: u8| {
            (0..(size as usize) * (size as usize))
                .flat_map(|texel| {
                    let step = (texel % 4) as u8;
                    [base, base + 1, base + 2, base.wrapping_add(step)]
                })
                .collect::<Vec<u8>>()
        };
        ProceduralBakeMaps::new(
            size,
            channel(fill),
            orm.then(|| channel(fill + 10)),
            normal.then(|| channel(fill + 20)),
        )
    }

    #[test]
    fn the_plan_bakes_the_shared_maps_before_any_material() {
        // index.js:82-83 — _tryBuild runs before the first getTextureSet.
        let plan = plan(Quality::Low, 64, &["asphalt", "brick"]);
        let order = plan.in_bake_order();
        assert_eq!(order.len(), 4);
        assert_eq!(order[0].key(), "__detail");
        assert_eq!(order[1].key(), "__macro");
        assert!(order[2].key().starts_with("asphalt|"));
        assert!(order[3].key().starts_with("brick|"));
        assert_eq!(plan.len(), 4);
        assert!(!plan.is_empty());
        // detail: albedo + height + Sobel = 3; macro: albedo = 1; two full
        // surfaces at 4 each.
        assert_eq!(plan.draw_count(), 3 + 1 + 4 + 4);
    }

    #[test]
    fn an_alias_collapses_onto_one_bake() {
        // `window` is an alias of `glass`; the bake key is what dedupes.
        let plan = plan(Quality::Low, 32, &["glass", "window", "rubber"]);
        assert_eq!(plan.surfaces.len(), 2, "three names, two bakes");
        assert!(plan.surfaces[0].key().starts_with("glass|"));
        assert!(plan.surfaces[1].key().starts_with("rubber|"));
    }

    #[test]
    fn the_plan_matches_the_cpu_bake_lists_keys_exactly() {
        // The GPU plan and `upload::bake_library` must plan the same bakes, or
        // the parity test is comparing two different libraries.
        let names = ["asphalt", "brick", "plaster", "window", "glass", "foliage"];
        let plan = plan(Quality::Low, 32, &names);
        let cpu = crate::materials::upload::bake_library(Quality::Low, 32, &names);
        let planned: Vec<&str> = plan.surfaces.iter().map(|r| r.key()).collect();
        let baked: Vec<&str> = cpu.surfaces.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(planned, baked);
    }

    #[test]
    fn the_shared_maps_carry_the_sources_own_constants() {
        let plan = plan(Quality::Ultra, u32::MAX, &[]);
        assert_eq!(plan.surfaces.len(), 0, "no names, no material bakes");
        assert_eq!(plan.detail.seed(), 1.0);
        assert_eq!(plan.detail.size(), 1024, "1K, not 512 — index.js:80-82");
        assert_eq!(plan.detail.world_size(), 0.25);
        assert_eq!(plan.detail.relief(), 0.0034);
        assert!(plan.detail.linear_albedo());
        assert!(!plan.detail.want_orm());
        assert!(plan.detail.want_normal());
        assert_eq!(plan.macro_field.seed(), 2.0);
        assert_eq!(plan.macro_field.size(), 256);
        assert_eq!(plan.macro_field.world_size(), 32.0);
        assert_eq!(plan.macro_field.relief(), 0.5);
        assert!(plan.macro_field.linear_albedo());
        assert!(!plan.macro_field.want_orm());
        assert!(
            !plan.macro_field.want_normal(),
            "the macro normal was baked and never sampled"
        );
    }

    #[test]
    fn a_material_bake_asks_for_all_three_maps_in_srgb() {
        let plan = plan(Quality::Low, 64, &["asphalt"]);
        let asphalt = &plan.surfaces[0];
        assert!(!asphalt.linear_albedo(), "a colour map is sRGB-encoded");
        assert!(asphalt.want_orm() && asphalt.want_normal());
        assert_eq!(asphalt.pass_count(), 4);
        assert!(asphalt.surface_wgsl().contains("fn owSurface"));
        assert_eq!(asphalt.surface_wgsl(), wgsl::ground::ASPHALT);
    }

    #[test]
    fn the_cap_never_upsamples_a_smaller_surface() {
        // `foliage` is authored at 512, the smallest the library uses.
        let plan = plan(Quality::Ultra, 4096, &["foliage"]);
        assert_eq!(plan.surfaces[0].size(), 512);
    }

    #[test]
    fn an_absent_tint_is_white_and_an_absent_param_is_zero() {
        assert_eq!(tint(None), [1.0, 1.0, 1.0]);
        assert_eq!(param(None), [0.0; 4]);
        assert_eq!(param(Some(&vec![1.0, 2.0])), [1.0, 2.0, 0.0, 0.0]);
        // 0xffffff is white in both spaces, so it pins the plumbing rather
        // than the curve; 0x808080 pins the curve.
        assert_eq!(tint(Some(0x00ff_ffff)), [1.0, 1.0, 1.0]);
        let mid = tint(Some(0x0080_8080));
        assert!(
            (mid[0] - 0.215_861_2).abs() < 1e-6,
            "three's SRGBToLinear of 128/255, got {}",
            mid[0]
        );
        assert_eq!(mid[0], mid[1], "a grey stays grey");
    }

    #[test]
    fn a_tinted_generator_carries_its_tint_and_param_through() {
        // `metal_painted` is the one generator that reads both uTintA and
        // uParam.z (a chip-amount bias).
        let plan = plan(Quality::Low, 64, &["metal_painted"]);
        let request = &plan.surfaces[0];
        assert_eq!(request.surface_wgsl(), wgsl::metal::METAL_PAINTED);
        assert!(
            request.tint_a() != [1.0, 1.0, 1.0] || request.param() != [0.0; 4],
            "the painted metal entry authors a tint or a param; got {:?} {:?}",
            request.tint_a(),
            request.param()
        );
    }

    #[test]
    fn the_orm_alpha_becomes_the_height_from_the_albedo() {
        let baked = maps(4, 100, true, true);
        let packed = surface_maps(&baked);
        (0..4).for_each(|x| {
            let orm = packed.orm_height.texel(x, 0);
            let albedo = packed.albedo.texel(x, 0);
            assert_eq!(orm[0], 110, "ORM keeps its own occlusion");
            assert_eq!(orm[3], albedo[3], "and takes the albedo's height");
        });
        assert_eq!(packed.normal.texel(1, 1)[0], 120);
    }

    #[test]
    fn the_detail_tile_is_the_normal_with_the_height_in_alpha() {
        let baked = maps(4, 50, false, true);
        let packed = detail_map(&baked);
        assert_eq!(packed.width, 4);
        (0..4).for_each(|x| {
            assert_eq!(packed.texel(x, 2)[0], 70, "the detail normal's x");
            assert_eq!(
                packed.texel(x, 2)[3],
                50_u8.wrapping_add(((2 * 4 + x) % 4) as u8),
                "and the micro height from the albedo's alpha"
            );
        });
    }

    #[test]
    fn assembling_pairs_every_key_with_its_own_texels() {
        let plan = plan(Quality::Low, 8, &["glass", "rubber"]);
        let answers = [maps(8, 1, true, true), maps(8, 60, true, true)];
        let library = assemble(
            &plan,
            &maps(8, 5, false, true),
            &maps(8, 9, false, false),
            &answers,
        );
        assert_eq!(library.surfaces.len(), 2);
        assert_eq!(library.surfaces[0].0, plan.surfaces[0].key());
        assert_eq!(library.surfaces[0].1.albedo.texel(0, 0)[0], 1);
        assert_eq!(library.surfaces[1].1.albedo.texel(0, 0)[0], 60);
        assert_eq!(library.detail.texel(0, 0)[0], 25, "5 + 20");
        assert_eq!(library.macro_field.texel(0, 0)[0], 9);
        assert!(library.bytes() > 0);
    }

    #[test]
    #[should_panic(expected = "asked for 2 surface bakes and got 1")]
    fn a_dropped_bake_is_a_panic_not_a_silent_mispairing() {
        let plan = plan(Quality::Low, 8, &["glass", "rubber"]);
        let _ = assemble(
            &plan,
            &maps(8, 5, false, true),
            &maps(8, 9, false, false),
            &[maps(8, 1, true, true)],
        );
    }

    #[test]
    #[should_panic(expected = "a per-material bake asks for ORM")]
    fn a_material_bake_without_orm_is_a_contract_violation() {
        let _ = surface_maps(&maps(4, 0, false, true));
    }

    #[test]
    #[should_panic(expected = "a per-material bake asks for a normal")]
    fn a_material_bake_without_a_normal_is_a_contract_violation() {
        let _ = surface_maps(&maps(4, 0, true, false));
    }

    #[test]
    #[should_panic(expected = "buildDetail bakes a normal")]
    fn a_detail_bake_without_a_normal_is_a_contract_violation() {
        let _ = detail_map(&maps(4, 0, false, false));
    }
}
