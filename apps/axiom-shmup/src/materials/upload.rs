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
///
/// **This lane is not what feeds parallax, and citing it as such is wrong.**
/// `material_shader::compose` samples binding 4 as `.rgb` only
/// (`compose.rs:283`), and `axiom_pom` is handed `albedo_tex`
/// (`compose.rs:266`) and marches `.a` of *that* (`pom.rs:137`). Since
/// [`super::bake::bake`] writes the height field into the **albedo's** alpha
/// (`bake.rs:327`), the height POM needs has been bound ever since the albedo
/// was uploaded — the ORM binding did not unlock it and its absence was never
/// what held it back. The copy stays because the engine documents binding 4's
/// alpha as the height lane and a future consumer is entitled to find it there;
/// it is simply unread today.
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

/// The two **independent** size caps a library bake takes.
///
/// These used to be one `u32` applied to both, and the conflation cost the
/// detail tile the one property its source states as a derivation rather than a
/// preference. They are different budgets in every way that matters:
///
/// | | bakes | `owSurface` evals per texel | measured at 256² |
/// |---|---|---|---|
/// | [`Self::surfaces`] | nineteen | 3 + a Sobel | see [`RUNTIME_BAKE_SIZE`] |
/// | [`Self::shared`] | **two** | 2 (detail) / 1 (macro) | 1146 ms |
///
/// A `u32` still converts (`impl From<u32>`), meaning "one cap, both budgets" —
/// which is what a CPU/GPU parity comparison wants, since
/// [`crate::materials::gpu_bake::plan`] must plan the identical library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BakeCaps {
    /// **Budget A** — the cap on each of the nineteen per-surface bakes.
    /// Quadratic in this number, nineteen times over. See [`RUNTIME_BAKE_SIZE`].
    pub surfaces: u32,
    /// **Budget B** — the cap on the two shared bakes (`build_detail`,
    /// `build_macro`). Quadratic in this number, twice. See
    /// [`SHARED_BAKE_SIZE`].
    pub shared: u32,
}

impl BakeCaps {
    /// One cap for both budgets — the historical meaning, and what a parity
    /// test wants when it needs the CPU bake and the GPU plan to be the same
    /// library.
    pub const fn uniform(size: u32) -> Self {
        Self {
            surfaces: size,
            shared: size,
        }
    }

    /// What the app actually installs at boot: [`RUNTIME_BAKE_SIZE`] on the
    /// nineteen, [`SHARED_BAKE_SIZE`] on the two.
    pub const RUNTIME: Self = Self {
        surfaces: RUNTIME_BAKE_SIZE,
        shared: SHARED_BAKE_SIZE,
    };
}

impl From<u32> for BakeCaps {
    fn from(size: u32) -> Self {
        Self::uniform(size)
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
/// own texture budget knob. [`BakeCaps`] clamps the result on top of it, with a
/// **separate cap per budget**: `surfaces` for the nineteen per-surface bakes,
/// `shared` for the two shared ones. A bare `u32` converts, meaning "one cap,
/// both budgets". `u32::MAX` means "whatever the library authored", which is the
/// faithful bake and, at 1024², **minutes** of CPU work (see
/// [`RUNTIME_BAKE_SIZE`] and [`SHARED_BAKE_SIZE`]).
/// The anisotropy value is the engine's own concern (`TextureSampling`), so it
/// is passed as the source's default 8 and read by nothing here.
pub fn bake_library(quality: Quality, caps: impl Into<BakeCaps>, names: &[&str]) -> BakedLibrary {
    let caps = caps.into();
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
        let size = set.size.min(caps.surfaces);
        surfaces.push((key, surface_maps(&set.bake_at(size, true, true))));
    }

    let shared = system
        .shared()
        .expect("configure with a renderer builds the shared maps (index.js:68-93)");
    // TWO BUDGETS, TWO CAPS. `caps.shared` rather than `caps.surfaces`, because
    // these are **one bake each** and the nineteen per-surface bakes are not.
    // Clamping them by the surface cap used to cost the detail tile the one
    // property its own source states as a derivation:
    //
    //   "1K, not 512: the micro tooth is 1.6-4 mm over a 0.25 m tile, which
    //    needs ~6 texels per grain to survive mip 1 instead of averaging to
    //    flat grey."  -- index.js:198-199, verified at HEAD
    //
    // See `SHARED_BAKE_SIZE` for the measured aliasing that conflation caused
    // and the measured cost of undoing it.
    let detail = super::bake::build_detail(shared.detail_size.min(caps.shared), 1.0);
    let macro_set = super::bake::build_macro(shared.macro_size.min(caps.shared), 2.0);
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
/// 57 million of them. The cause is structural, not a missing optimisation.
/// (The attribution in this doc used to name `ow_hash22` as "the classic
/// `fract(sin(dot(…)))` GLSL hash". That is wrong and
/// [`crate::materials::gpu_bake`] corrects it: `noise.js:11` says *"hashes are
/// sin-free (Dave Hoskins style)"*. The transcendentals are in `owGrad2` — a
/// `cos` and a `sin` per lattice corner, four corners per `owNoise`, four
/// octaves per fbm, so 32 per four-octave fbm. The measurements and the
/// conclusion stand; only the attribution was wrong.) No CPU rewrite closes a
/// 100x gap that is really 1024²-way parallelism.
///
/// So the boot-time bake runs at this cap. 64² over a 2 m tile is 3 cm per
/// texel: coarse, but it is the real generator's colour field, and the material
/// shader's macro, weathering and cavity layers — which are per-pixel and cost
/// nothing here — carry the high frequencies on top of it.
///
/// **The fix is the source's own**: bake on the GPU. `bake.rs`'s module doc
/// already spells out what that needs (WGSL emission of `owSurface`, a
/// half-float scratch height target, the Sobel as a fragment shader), and
/// `sobel` is written to be line-for-line portable to it. Until then this
/// constant is the honest ceiling, and raising it costs boot time
/// quadratically.
///
/// ## The MEASURED cost curve
///
/// One `bake_library` call at `caps.surfaces = N` costs, per distinct bake key,
/// `bake_at(N, want_orm: true, want_normal: true)` = **three** `owSurface`
/// evaluations per texel (height, albedo, ORM) plus one Sobel. Nineteen
/// distinct keys, quadratic in `N`.
///
/// This table used to be an **extrapolation** from a single 512² measurement.
/// It is now MEASURED by `probe::measure_bake_cost_curve`, native, `--release`,
/// `stable-x86_64-pc-windows-msvc`, with the two shared bakes timed separately
/// and subtracted out so this is Budget A alone:
///
/// ```text
/// N     surfaces_ms   per_key_ms     the old extrapolation said
/// 64          6172          325                          3600
/// 96         16320          859                          8200
/// 128        19524         1028                         14500
/// 192        39874         2099                         32600
/// ```
///
/// **The extrapolation was optimistic at every point**, by 1.2x to 2.0x —
/// exactly the direction its own caveat warned about. Two honest qualifications
/// on the new numbers: other agents' `rustc` processes were resident during the
/// run, so these are contended upper bounds; and the `96` point is visibly
/// noisy against `128` (0.84x the time for 0.56x the area), which is that
/// contention showing. The `64` figure — **~6.2 s** — is the one that matters,
/// and it is the cost this app pays at every boot today.
///
/// ## Why this stays at 64 while [`SHARED_BAKE_SIZE`] moved
///
/// The two budgets were measured against the defect, not just against the
/// clock. `scripts/parity_metrics.py --region ground` (the street surface,
/// rows 55-88%) reports the port's **gradient energy at 1.025x** the
/// original's — the per-surface maps are already carrying very nearly the right
/// amount of surface detail. Raising `N` to 128 buys an unproven improvement on
/// that for a measured **+13.3 s** of boot. The shared detail tile, by
/// contrast, was measurably *aliased* rather than merely coarse
/// (see [`SHARED_BAKE_SIZE`]), and fixing it cost **+1.1 s**.
///
/// That is the whole argument for splitting the knob: at 64 the nineteen
/// surface bakes were **99%** of the bake budget (6172 ms against 64 ms) while
/// the two shared bakes — the ones that needed the resolution — got the same
/// cap for 1% of the cost.
///
/// **Both constants should be deleted, not tuned, once the wasm GPU-bake lane
/// lands.** The source pays ~1.3 s for all nineteen and 418 ms for the shared
/// pair, on the GPU, at the *full authored* resolutions. See
/// [`crate::materials::gpu_bake`] for the lane and what it still needs.
pub const RUNTIME_BAKE_SIZE: u32 = 64;

/// **Budget B**: the cap on the two *shared* bakes — `build_detail` and
/// `build_macro`. Split out of [`RUNTIME_BAKE_SIZE`], which used to cap both
/// budgets through one number.
///
/// ## The source's authored sizes, and their derivation
///
/// `index.js:198-199` (verified at HEAD) does not merely state a number, it
/// derives one:
///
/// > *"1K, not 512: the micro tooth is 1.6-4 mm over a 0.25 m tile, which needs
/// > ~6 texels per grain to survive mip 1 instead of averaging to flat grey."*
///
/// So the authored pair is **detail 1024²**, **macro 256²**
/// (`this._size(1024)` and a literal `256`). The derivation checks out exactly:
/// over a 0.25 m tile, texels per 1.6 mm grain is `N / 156.25`, and `N = 1024`
/// gives **6.55** — the "~6" is that arithmetic, not a preference.
///
/// ```text
/// N      mm/texel   texels per 1.6 mm grain
/// 64      3.906      0.41      <- 5x below Nyquist
/// 128     1.953      0.82
/// 256     0.977      1.64
/// 512     0.488      3.28      <- first size above Nyquist
/// 1024    0.244      6.55      <- the source's authored size
/// ```
///
/// ## Why the old cap of 64 was not "coarse", it was **aliased**
///
/// `bake` evaluates the surface at exactly one point per texel — no
/// supersampling. Below Nyquist that does not produce a coarse average of the
/// field, it produces fold-back: full-amplitude noise uncorrelated with the
/// signal. MEASURED by `probe::measure_detail_bake_aliasing`, against a 512²
/// bake box-averaged down to the same size (the band-limited answer):
///
/// ```text
/// N     height channel: sd(direct)/sd(band-limited)    rms error / sd
/// 64                 2.615                                 2.292
/// 128                1.664                                 1.150
/// 256                1.232                                 0.615
/// ```
///
/// At 64 the tile carried **2.6x the variance it should**, with an error
/// **2.3x the signal's own spread** — more fold-back than field. That is a
/// broadband high-frequency injector, and it is worse than a low resolution,
/// not better: at 3.9 mm/texel over a 0.25 m tile the map is *magnified* on the
/// near ground (~1 texel per 1.4 px), so it is sampled at mip 0 where no
/// filtering exists to remove it. Raising the cap both cuts the fold-back at
/// bake time and pushes the tile into *minification*, where the mip chain
/// `scene_renderer::upload_texture` builds does band-limit it.
///
/// ## What this port can afford
///
/// MEASURED by `probe::measure_shared_bake_cost`, native, `--release`,
/// `stable-x86_64-pc-windows-msvc` (the two bakes, wall clock):
///
/// ```text
/// N      detail_ms    macro_ms      sum_ms
/// 64          43.6        20.2        63.8
/// 128        199.3       112.0       311.3
/// 256        801.0       345.2      1146.2
/// 512       2969.1       371.9      3341.0
/// 1024     10305.6       332.7     10638.3
/// ```
///
/// (`macro` is authored at 256, so from `N = 256` up it is uncapped and flat.)
///
/// **256 is the value here, and it is MEASURED, not chosen for looking right:**
///
/// * it is the macro field's **full authored size**, so for one of the two maps
///   the cap is retired entirely and the port is at source parity;
/// * it is **one quarter** of the detail tile's authored 1024 in each axis, and
///   it cuts that tile's measured aliasing excess from 2.6x to 1.2x;
/// * it costs **+1082 ms** native over the old 64 (1146 against 64), against
///   **+3277 ms** for 512 and **+10574 ms** for the authored 1024.
///
/// The remaining gap to 1024 is bought entirely by CPU time, and the source
/// pays none of it: on the GPU the same two bakes cost it **418 ms** of cold
/// shader compile (`index.js:200-202`), which is why it can afford the full
/// resolution and this port cannot. **The gap closes completely when the wasm
/// GPU-bake lane lands** — see [`crate::materials::gpu_bake`], which already
/// stages the request shapes and holds the CPU/GPU parity test. At that point
/// this constant and [`RUNTIME_BAKE_SIZE`] should both be deleted rather than
/// raised: the correct value is "whatever the library authored", and only the
/// CPU cost is stopping it.
pub const SHARED_BAKE_SIZE: u32 = 256;


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


// ---------------------------------------------------------------------------
// PROBES — the measured cost curve and sampling error behind the two caps.
// ---------------------------------------------------------------------------

/// Measurements, not assertions.
///
/// Every test here is `#[ignore]`d and prints a table: they exist so the
/// numbers in [`RUNTIME_BAKE_SIZE`]'s and [`SHARED_BAKE_SIZE`]'s docs are
/// *measured* rather than extrapolated, and so the next agent to move either
/// constant can re-measure instead of re-deriving. Run them one at a time:
///
/// ```sh
/// RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc cargo test -p axiom-shmup \
///     --lib --release probe:: -- --ignored --nocapture --test-threads=1
/// ```
#[cfg(test)]
mod probe {
    use super::*;
    use crate::materials::bake::{build_detail, build_macro, Texture};
    use crate::world::palette::Palette;
    use std::time::Instant;

    /// The exact name list `scene::install` bakes — so the probe measures the
    /// real library, not a sample of it.
    fn street_names() -> Vec<&'static str> {
        Palette::ALL
            .iter()
            .map(|(_, entry)| entry.name)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn ms(d: std::time::Duration) -> f64 {
        d.as_secs_f64() * 1e3
    }

    /// **Budget A and Budget B, separated.** One `bake_library` call at each
    /// candidate cap, with the two shared bakes timed on their own so the
    /// nineteen-surface cost can be read apart from them.
    #[test]
    #[ignore = "measurement: minutes of CPU"]
    fn measure_bake_cost_curve() {
        let names = street_names();
        let mut system = MaterialSystem::new(Some(RendererCaps {
            max_anisotropy: Some(8.0),
        }));
        system.configure(Quality::Ultra, 8);
        let shared = system.shared().expect("configure builds the shared maps");
        println!(
            "names={} authored detail_size={} macro_size={}",
            names.len(),
            shared.detail_size,
            shared.macro_size
        );
        println!("  N   keys      total_ms    detail_ms     macro_ms  surfaces_ms   per_key_ms");
        for n in [64u32, 96, 128, 192] {
            let t = Instant::now();
            let lib = bake_library(Quality::Ultra, n, &names);
            let total = ms(t.elapsed());

            let t = Instant::now();
            let _ = build_detail(shared.detail_size.min(n), 1.0);
            let detail = ms(t.elapsed());

            let t = Instant::now();
            let _ = build_macro(shared.macro_size.min(n), 2.0);
            let macro_ = ms(t.elapsed());

            let surfaces = total - detail - macro_;
            println!(
                "{:4}  {:4}  {:12.1} {:12.1} {:12.1} {:12.1} {:12.1}",
                n,
                lib.surfaces.len(),
                total,
                detail,
                macro_,
                surfaces,
                surfaces / lib.surfaces.len() as f64
            );
        }
    }

    /// **Budget B alone**, across the range the shared cap could take. Two
    /// bakes, so this is the curve that decides [`SHARED_BAKE_SIZE`].
    #[test]
    #[ignore = "measurement: minutes of CPU"]
    fn measure_shared_bake_cost() {
        println!("  N     detail_ms     macro_ms       sum_ms");
        for n in [64u32, 128, 256, 512, 1024] {
            let t = Instant::now();
            let _ = build_detail(n, 1.0);
            let detail = ms(t.elapsed());

            let t = Instant::now();
            let _ = build_macro(n.min(256), 2.0);
            let macro_ = ms(t.elapsed());

            println!(
                "{:4}  {:12.1} {:12.1} {:12.1}",
                n,
                detail,
                macro_,
                detail + macro_
            );
        }
    }

    /// Box-average `src` down to `to` texels a side — the band-limited answer a
    /// correctly-sampled bake at `to` would have produced.
    fn downsample(src: &Texture, to: u32) -> Vec<[f32; 4]> {
        let f = src.size / to;
        let n = f64::from(f * f);
        (0..to)
            .flat_map(|y| (0..to).map(move |x| (x, y)))
            .map(|(x, y)| {
                let mut acc = [0f64; 4];
                for j in 0..f {
                    for i in 0..f {
                        let p = src.get(x * f + i, y * f + j);
                        for c in 0..4 {
                            acc[c] += f64::from(p[c]);
                        }
                    }
                }
                [
                    (acc[0] / n) as f32,
                    (acc[1] / n) as f32,
                    (acc[2] / n) as f32,
                    (acc[3] / n) as f32,
                ]
            })
            .collect()
    }

    fn channel(texels: &[[f32; 4]], c: usize) -> Vec<f64> {
        texels.iter().map(|t| f64::from(t[c])).collect()
    }

    fn std_dev(v: &[f64]) -> f64 {
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
    }

    fn rms_diff(a: &[f64], b: &[f64]) -> f64 {
        (a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f64>()
            / a.len() as f64)
            .sqrt()
    }

    /// **Is the detail tile aliased at bake time, or merely coarse?**
    ///
    /// `build_detail` evaluates `detail_surface` at ONE point per texel — no
    /// supersampling — over a 0.25 m tile carrying a 1.6 mm grain. At 64 texels
    /// the sample spacing is 3.9 mm, so the grain sits well inside the fold-back
    /// region and what lands in the texture is not a coarse average of the field
    /// but an aliased draw from it.
    ///
    /// This measures the difference. `reference` is a 512² bake box-averaged
    /// down to `n` — the band-limited answer. `direct` is the bake this port
    /// actually ships at `n`. Two numbers matter per channel:
    ///
    /// * **`rms/sd`** — the sampling error as a fraction of the channel's own
    ///   spread. Near 0 means "coarse but correct"; near or past 1 means most of
    ///   what is in the tile at that resolution is fold-back.
    /// * **`sd_ratio`** — the direct bake's standard deviation over the
    ///   band-limited one's. A correctly band-limited downsample *loses*
    ///   variance; an undersampled one *keeps* it. A ratio well above 1 is
    ///   aliasing showing up as exactly the excess high-frequency energy the
    ///   parity metric measures.
    #[test]
    #[ignore = "measurement: minutes of CPU"]
    fn measure_detail_bake_aliasing() {
        let reference = build_detail(512, 1.0);
        let ref_normal = reference
            .normal
            .as_ref()
            .expect("build_detail bakes a normal");
        println!("  N  channel               sd_direct   sd_bandlim    sd_ratio     rms   rms/sd");
        for n in [64u32, 128, 256] {
            let direct = build_detail(n, 1.0);
            let direct_normal = direct.normal.as_ref().expect("normal");
            // albedo.a is the height field; albedo.r is the micro albedo.
            let rows: [(&str, &Texture, &Texture, usize); 3] = [
                ("height (albedo.a)", &reference.albedo, &direct.albedo, 3),
                ("albedo  (albedo.r)", &reference.albedo, &direct.albedo, 0),
                ("normal  (normal.r)", ref_normal, direct_normal, 0),
            ];
            for (label, reference_tex, direct_tex, c) in rows {
                let band = channel(&downsample(reference_tex, n), c);
                let dir = channel(&direct_tex.texels, c);
                let (sd_d, sd_b) = (std_dev(&dir), std_dev(&band));
                let rms = rms_diff(&dir, &band);
                println!(
                    "{:4}  {:20} {:9.5} {:12.5} {:11.3} {:9.5} {:8.3}",
                    n,
                    label,
                    sd_d,
                    sd_b,
                    sd_d / sd_b,
                    rms,
                    rms / sd_b
                );
            }
        }
    }
}
/// **Where does the road's sand cast come from?**
///
/// The parity capture reads the original's road as neutral grey `[104,102,94]`
/// (1.00 : 0.98 : 0.90) and the port's as sand `[104,71,45]`
/// (1.00 : 0.68 : 0.43). Those are *rendered* pixels — albedo times tint times
/// the weathering/dust layer times lighting times grade — so the triple alone
/// cannot say which stage introduced the warmth.
///
/// This isolates the first stage: the mean sRGB-encoded texel of each ground
/// surface's baked albedo, in 0-255. A neutral mean here clears
/// `materials/surfaces/` and moves the question downstream to the palette tint,
/// the shader's weathering/dust layer, or the grade.
#[cfg(test)]
mod probe_colour {
    use super::*;

    #[test]
    #[ignore = "measurement"]
    fn measure_baked_surface_colour() {
        let mut system = MaterialSystem::new(Some(RendererCaps {
            max_anisotropy: Some(8.0),
        }));
        system.configure(Quality::Ultra, 8);
        let opts = MaterialOpts::new();
        println!("surface        mean_r mean_g mean_b     R:G:B (normalised to R)");
        for name in ["asphalt", "sand", "concrete", "dirt", "gravel"] {
            let Some(key) = system.texture_set_key(name, &opts) else {
                println!("{name:14} <unresolved>");
                continue;
            };
            let set = system.texture_set(&key).expect("just inserted");
            let baked = set.bake_at(128, false, false);
            let n = baked.albedo.texels.len() as f64;
            let mut acc = [0f64; 3];
            for t in &baked.albedo.texels {
                for c in 0..3 {
                    acc[c] += f64::from(t[c]);
                }
            }
            let m = [acc[0] / n, acc[1] / n, acc[2] / n];
            println!(
                "{:14} {:6.1} {:6.1} {:6.1}     1.000 : {:.3} : {:.3}   [key {key}]",
                name,
                m[0] * 255.0,
                m[1] * 255.0,
                m[2] * 255.0,
                m[1] / m[0].max(1e-9),
                m[2] / m[0].max(1e-9),
            );
        }
    }
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
