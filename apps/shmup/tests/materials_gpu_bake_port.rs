//! The GPU bake against the CPU goldens.
//!
//! Three things are joined here, and this test file is the only place that may
//! name all three, because it is the only place at the app tier:
//!
//! 1. `axiom_shmup::materials::wgsl` — the nineteen generators as WGSL,
//!    transcribed from `glsl/surfaces-*.js` and `glsl/noise.js`;
//! 2. `axiom_shmup::materials::gpu_bake` — the bake list, ported from
//!    `index.js`;
//! 3. `axiom_gpu_backend`'s `GpuBackendApi::bake_procedural_texture` — the
//!    forge, ported from `generator.js`.
//!
//! The oracle is `materials::upload::bake_library`, the CPU bake, which is
//! already pinned against captures from the original JavaScript
//! (`materials_upload_port.rs`, `materials_surfaces_*_port.rs`,
//! `materials_noise_port.rs`). **A GPU bake that disagrees with it is wrong**
//! — subject to the divergence budget below, which is derived from the two
//! sides' arithmetic and not from any miss observed here.
//!
//! ## THE TOLERANCES BELOW ARE PREDICTIONS, NOT MEASUREMENTS
//!
//! This test was written in a wave that forbade building and running, so it has
//! **never executed**. Every figure in `Budget` is derived from the two
//! implementations' numerics, stated so it can be checked, and expected to be
//! replaced by a measured one on the first green run. Treat a failure as
//! information about the budget until the budget has been measured once.
//!
//! ## Where the divergence comes from, in order of size
//!
//! **1. `f64` CPU against `f32` GPU — the dominant term.** `materials::noise`
//! is `f64` throughout, deliberately (`noise.rs:29-34` says the WGSL workstream
//! "is the point at which `f32` truncation becomes an explicit, separately
//! tested concern" — this is that point). The Dave-Hoskins hashes take `fract`
//! of intermediates around 30–100, where an `f32` ulp is ~7.6e-6, so a hash
//! output carries ~1e-5 of absolute error that `f64` does not. Through
//! `owGrad2`'s `cos`/`sin`, four corners and four octaves, a fbm lands around
//! 1e-4. Carried into a colour and sRGB-encoded (whose derivative peaks near
//! black at ~2.7), that is **≈0.07 of one 8-bit LSB** — invisible.
//!
//! **2. Hard `step()` edges and Worley F1/F2 ties — the outliers.** A `step()`
//! is discontinuous, so where the two sides straddle its edge the output jumps
//! by that term's whole contribution (`gritA * 0.26` on a height is 66 LSB).
//! With ~1e-5 of disagreement and hash values roughly uniform, roughly 1e-5 of
//! texels flip per `step` call site; the library has tens. A `d < f1` tie in
//! `owWorley` swaps the cell `id` outright and is the same story. These texels
//! are bounded in **count**, not in magnitude, which is why the budget below
//! caps a fraction rather than a maximum. Anything else would be a tolerance
//! fitted to the miss.
//!
//! **3. The half-float height scratch — negligible, and deliberate.** The
//! source bakes height into a `HalfFloatType` target (`generator.js:180-186`)
//! and the GPU path is faithful to that; the CPU port keeps `f32` and says so.
//! An `f16` round-off of ±1.2e-4 at h≈0.5, through a Sobel of weight 8 scaled
//! by `0.125` and then by `size * relief / worldSize`, is ≈5e-4 on a unit
//! normal: **≈0.07 LSB**. It is listed because it is a real, named,
//! *intentional* divergence, not because it is large.
//!
//! Both maps' 8-bit quantisation then adds a ±0.5 LSB step of its own wherever
//! the two sides land either side of a rounding boundary — which is why the
//! per-texel allowance is 2 LSB and not 1.

use axiom_gpu_backend::GpuBackendApi;
use axiom_host::{ProceduralBakeMaps, ProceduralBakeRequest};
use axiom_shmup::config::Quality;
use axiom_shmup::materials::gpu_bake::{self, GpuBakePlan};
use axiom_shmup::materials::upload::{self, BakedLibrary, Rgba8Map};
use axiom_shmup::materials::wgsl;

/// The size the whole-library sweep bakes at.
///
/// Small because the *CPU* side is the expensive one: `bake_library` evaluates
/// every surface three times per texel at ~15.5 µs an evaluation, so nineteen
/// surfaces at 32² is ~0.9 s in `--release` and 512² would be four minutes. The
/// GPU side does not care. 32 is also deliberately **not** a multiple of 64, so
/// `32 * 4 = 128` bytes a row exercises the read-back's 256-byte row padding
/// rather than skipping it.
const SWEEP_SIZE: u32 = 32;

/// Every name the street's palette resolves to, one per generator body plus the
/// two `concrete` variants — the nineteen the source bakes.
const LIBRARY_NAMES: [&str; 19] = [
    "concrete",
    "concrete_floor",
    "brick",
    "plaster",
    "tile",
    "asphalt",
    "sand",
    "dirt",
    "gravel",
    "metal_rust",
    "metal_painted",
    "metal_brushed",
    "corrugated",
    "wood",
    "fabric",
    "burlap",
    "foliage",
    "rubber",
    "glass",
];

/// The predicted divergence budget. See the module doc: **unverified**.
struct Budget {
    /// Mean absolute per-channel difference, in 8-bit LSB. Driven by term (1),
    /// which is ~0.07 LSB, plus the ±0.5 LSB quantisation step wherever the two
    /// sides straddle a rounding boundary — which happens for a small fraction
    /// of texels, so the mean stays well under one.
    mean_lsb: f64,
    /// Per-channel allowance before a texel counts as an outlier: one
    /// quantisation step plus one for the sRGB encode's slope near black.
    texel_lsb: u8,
    /// Fraction of channels allowed past `texel_lsb`. Term (2): ~1e-5 per
    /// `step`/tie site, tens of sites, rounded up by two orders of magnitude
    /// for the ones that sit near a threshold by construction.
    outlier_fraction: f64,
}

/// Albedo and ORM: continuous functions of the surface, sampled once.
const SURFACE_BUDGET: Budget = Budget {
    mean_lsb: 0.75,
    texel_lsb: 2,
    outlier_fraction: 0.002,
};

/// The normal map: a *derivative* of the height field, so every divergence in
/// the height is amplified by `size * relief / worldSize` before it reaches a
/// texel, and each outlier texel of term (2) smears across its 3x3
/// neighbourhood.
const NORMAL_BUDGET: Budget = Budget {
    mean_lsb: 1.5,
    texel_lsb: 4,
    outlier_fraction: 0.02,
};

#[derive(Debug, Clone, Copy, Default)]
struct Stats {
    channels: usize,
    total_delta: u64,
    max_delta: u8,
    outliers: usize,
    /// The range the GPU's own bytes span — the anti-vacuity guard. Two flat
    /// maps agree perfectly and prove nothing.
    spread: u8,
}

impl Stats {
    fn mean_lsb(self) -> f64 {
        self.total_delta as f64 / self.channels.max(1) as f64
    }

    fn outlier_fraction(self) -> f64 {
        self.outliers as f64 / self.channels.max(1) as f64
    }
}

fn compare(cpu: &Rgba8Map, gpu: &Rgba8Map, allowance: u8) -> Stats {
    assert_eq!(cpu.width, gpu.width, "the two bakes must be the same size");
    assert_eq!(cpu.height, gpu.height, "the two bakes must be the same size");
    assert_eq!(cpu.pixels.len(), gpu.pixels.len());
    let (mut lo, mut hi) = (255_u8, 0_u8);
    let stats = cpu.pixels.iter().zip(gpu.pixels.iter()).fold(
        Stats::default(),
        |mut stats, (expected, actual)| {
            let delta = expected.abs_diff(*actual);
            stats.channels += 1;
            stats.total_delta += u64::from(delta);
            stats.max_delta = stats.max_delta.max(delta);
            stats.outliers += usize::from(delta > allowance);
            lo = lo.min(*actual);
            hi = hi.max(*actual);
            stats
        },
    );
    Stats {
        spread: hi.saturating_sub(lo),
        ..stats
    }
}

fn assert_within(label: &str, budget: &Budget, stats: Stats) {
    assert!(
        stats.spread > 8,
        "{label} is nearly flat (spread {} LSB) — a comparison of two constants \
         proves nothing about the transcription",
        stats.spread
    );
    assert!(
        stats.mean_lsb() <= budget.mean_lsb,
        "{label}: mean |CPU - GPU| is {:.4} LSB, budget {:.4}. \
         (max {} LSB, {} of {} channels past {} LSB = {:.5})",
        stats.mean_lsb(),
        budget.mean_lsb,
        stats.max_delta,
        stats.outliers,
        stats.channels,
        budget.texel_lsb,
        stats.outlier_fraction()
    );
    assert!(
        stats.outlier_fraction() <= budget.outlier_fraction,
        "{label}: {} of {} channels differ by more than {} LSB ({:.5}), budget \
         {:.5}. Mean {:.4} LSB, max {} LSB. A step()/Worley-tie flip is expected \
         at ~1e-5 a site; a fraction this high is a transcription defect, not \
         float drift — find the site before widening the budget.",
        stats.outliers,
        stats.channels,
        budget.texel_lsb,
        stats.outlier_fraction(),
        budget.outlier_fraction,
        stats.mean_lsb(),
        stats.max_delta
    );
}

fn run(request: &ProceduralBakeRequest, library: &str) -> ProceduralBakeMaps {
    GpuBackendApi::bake_procedural_texture(library, request).unwrap_or_else(|| {
        panic!(
            "the GPU bake of {} needs a real adapter; there is no honest fallback",
            request.key()
        )
    })
}

fn bake_plan(plan: &GpuBakePlan) -> BakedLibrary {
    let library = wgsl::library_wgsl();
    let detail = run(&plan.detail, &library);
    let macro_field = run(&plan.macro_field, &library);
    let surfaces: Vec<ProceduralBakeMaps> = plan
        .surfaces
        .iter()
        .map(|request| run(request, &library))
        .collect();
    gpu_bake::assemble(plan, &detail, &macro_field, &surfaces)
}

/// The whole street, GPU against CPU, every map.
///
/// This is the test the slice exists for. It is one test rather than nineteen
/// because the CPU side is the cost and it is paid once.
#[test]
fn every_generator_agrees_with_its_cpu_golden() {
    let plan = gpu_bake::plan(Quality::Ultra, SWEEP_SIZE, &LIBRARY_NAMES);
    let gpu = bake_plan(&plan);
    let cpu = upload::bake_library(Quality::Ultra, SWEEP_SIZE, &LIBRARY_NAMES);

    assert_eq!(
        gpu.surfaces.len(),
        cpu.surfaces.len(),
        "the two bakes must plan the same library"
    );
    gpu.surfaces
        .iter()
        .zip(cpu.surfaces.iter())
        .for_each(|((gpu_key, gpu_maps), (cpu_key, cpu_maps))| {
            assert_eq!(gpu_key, cpu_key, "the bake order must match");
            assert_within(
                &format!("{gpu_key} albedo"),
                &SURFACE_BUDGET,
                compare(&cpu_maps.albedo, &gpu_maps.albedo, SURFACE_BUDGET.texel_lsb),
            );
            assert_within(
                &format!("{gpu_key} orm+height"),
                &SURFACE_BUDGET,
                compare(
                    &cpu_maps.orm_height,
                    &gpu_maps.orm_height,
                    SURFACE_BUDGET.texel_lsb,
                ),
            );
            assert_within(
                &format!("{gpu_key} normal"),
                &NORMAL_BUDGET,
                compare(&cpu_maps.normal, &gpu_maps.normal, NORMAL_BUDGET.texel_lsb),
            );
        });

    assert_within(
        "__detail",
        &NORMAL_BUDGET,
        compare(&cpu.detail, &gpu.detail, NORMAL_BUDGET.texel_lsb),
    );
    assert_within(
        "__macro",
        &SURFACE_BUDGET,
        compare(&cpu.macro_field, &gpu.macro_field, SURFACE_BUDGET.texel_lsb),
    );
}

/// Isolate the noise library from the generators.
///
/// If this fails, the defect is in `wgsl::noise`; if it passes and
/// `every_generator_agrees_with_its_cpu_golden` fails, the defect is in one
/// `surfaces-*.js` transcription. Without this split, a hash typo shows up as
/// eighteen simultaneous failures and reads as a tolerance problem.
///
/// The probe writes four library outputs into the four channels of a linear
/// (un-encoded) albedo target, so each byte is `round(clamp(f, 0, 1) * 255)` of
/// the value itself.
#[test]
fn the_noise_library_agrees_with_its_cpu_twin() {
    use axiom_shmup::materials::noise::{ow_fbm01, ow_hash12, ow_voronoi_edge, ow_worley, Vec2};

    const PROBE: &str = r#"
fn owSurface(uv: vec2<f32>, albOut: ptr<function, vec3<f32>>, hOut: ptr<function, f32>, roughOut: ptr<function, f32>, metalOut: ptr<function, f32>, aoOut: ptr<function, f32>) {
  let P = vec2<f32>(8.0);
  let p = uv * P;
  *albOut = vec3<f32>(owHash12(p * 37.0),
                      owFbm01(p * 3.0, P * 3.0, 4, 0.55),
                      owWorley(p * 5.0, P * 5.0, 1.0).x);
  *hOut = owVoronoiEdge(p * 4.0, P * 4.0, 1.0);
  *roughOut = 0.5;
  *metalOut = 0.0;
  *aoOut = 1.0;
}
"#;

    let size = 32_u32;
    let request =
        ProceduralBakeRequest::new("__noise_probe".to_string(), PROBE.to_string(), size)
            // The probe's four channels are DATA, not colour: no sRGB encode,
            // so a byte is the value.
            .with_linear_albedo(true)
            .with_maps(false, false);
    let gpu = run(&request, &wgsl::library_wgsl());

    let quantize = |value: f64| (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let per = Vec2::splat(8.0);
    let stats = (0..size).fold(Stats::default(), |stats, y| {
        (0..size).fold(stats, |mut stats, x| {
            let uv = Vec2::new(
                (f64::from(x) + 0.5) / f64::from(size),
                (f64::from(y) + 0.5) / f64::from(size),
            );
            let p = uv.mul(per);
            let expected = [
                quantize(ow_hash12(p.scale(37.0))),
                quantize(ow_fbm01(p.scale(3.0), per.scale(3.0), 4, 0.55)),
                quantize(ow_worley(p.scale(5.0), per.scale(5.0), 1.0).f1),
                quantize(ow_voronoi_edge(p.scale(4.0), per.scale(4.0), 1.0)),
            ];
            let at = ((y * size + x) * 4) as usize;
            (0..4).for_each(|lane| {
                let delta = expected[lane].abs_diff(gpu.albedo()[at + lane]);
                stats.channels += 1;
                stats.total_delta += u64::from(delta);
                stats.max_delta = stats.max_delta.max(delta);
                stats.outliers += usize::from(delta > 2);
                stats.spread = stats.spread.max(gpu.albedo()[at + lane]);
            });
            stats
        })
    });
    assert_within(
        "noise probe (hash12 / fbm01 / worley.F1 / voronoiEdge)",
        &SURFACE_BUDGET,
        stats,
    );
}

/// Every generator's WGSL compiles and produces finite, in-range texels.
///
/// Cheap (4² a surface) and it fires before the expensive sweep does, so a
/// parse error in one transcription is reported as a parse error rather than as
/// a missing library entry.
#[test]
fn every_generator_compiles_and_bakes() {
    let library = wgsl::library_wgsl();
    wgsl::GENERATOR_NAMES.iter().for_each(|name| {
        let request = ProceduralBakeRequest::new(
            (*name).to_string(),
            wgsl::generator_wgsl(name).to_string(),
            4,
        )
        .with_seed(1.0)
        .with_tints([0.5, 0.5, 0.5], [0.5, 0.5, 0.5]);
        let maps = run(&request, &library);
        assert_eq!(maps.size(), 4);
        assert_eq!(maps.albedo().len(), 4 * 4 * 4, "{name} albedo");
        assert!(maps.orm().is_some(), "{name} ORM");
        assert!(maps.normal().is_some(), "{name} normal");
        // A NaN through the fixed-point conversion lands as 0; an all-zero
        // albedo across every texel is the shape that finds it.
        assert!(
            maps.albedo()
                .chunks_exact(4)
                .any(|texel| texel[..3] != [0, 0, 0]),
            "{name} baked a fully black albedo — a NaN in the generator reaches \
             the 8-bit write as zero"
        );
    });
}

/// The bake is deterministic: the same request twice is byte-identical.
///
/// Not a given on a GPU — an uninitialised attachment, or a missing dependency
/// between the height pass and the Sobel that reads it, would show up here and
/// nowhere else.
#[test]
fn the_same_request_bakes_the_same_bytes_twice() {
    let plan = gpu_bake::plan(Quality::Low, 32, &["asphalt"]);
    let library = wgsl::library_wgsl();
    let first = run(&plan.surfaces[0], &library);
    let second = run(&plan.surfaces[0], &library);
    assert_eq!(first, second, "the bake must be replayable byte for byte");
}

/// The GPU plan and the CPU bake ask for the same nineteen bakes, in the same
/// order — checked without a device, so a plan drift is caught even on a
/// machine with no adapter.
#[test]
fn the_gpu_plan_matches_the_cpu_bake_list() {
    let plan = gpu_bake::plan(Quality::Ultra, u32::MAX, &LIBRARY_NAMES);
    assert_eq!(plan.surfaces.len(), 19, "the source bakes nineteen");
    assert_eq!(plan.len(), 21, "plus the two shared maps");
    let cpu = upload::bake_library(Quality::Low, 4, &LIBRARY_NAMES);
    // The keys carry the size, which `Low` halves, so compare the names.
    let name_of = |key: &str| key.split('|').next().unwrap_or("").to_string();
    assert_eq!(
        plan.surfaces
            .iter()
            .map(|r| name_of(r.key()))
            .collect::<Vec<_>>(),
        cpu.surfaces
            .iter()
            .map(|(key, _)| name_of(key))
            .collect::<Vec<_>>()
    );
}
