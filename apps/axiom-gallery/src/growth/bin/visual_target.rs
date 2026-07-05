//! `visual-target` — the Axiom Visual Target 001 runner.
//!
//! Loads one fixed, versioned scene manifest, renders **exactly one frame** through
//! a chosen backend, saves a PNG, and can compare that render against a reference
//! image. Native + `visual-target` feature only (`required-features`), so it never
//! enters the wasm bundle or the default gates.
//!
//! ```text
//! visual-target render  <scene.toml> [--backend gpu|canvas2d] [--out PATH]
//! visual-target bless   <scene.toml> [--backend gpu|canvas2d] [--out PATH]
//! visual-target compare <scene.toml> <reference.png> [--backend ...] [--tol MEAN,MAX] [--diff PATH]
//! ```
//!
//! * **render** — render + save a PNG (default `screenshots/<name>[_canvas2d].png`).
//! * **bless** — render + save the *reference* PNG (default next to the scene as
//!   `<stem>[.canvas2d].reference.png`).
//! * **compare** — render + diff against a reference; prints stats + a PASS/FAIL
//!   verdict and exits non-zero on failure (CI-usable). Canvas 2D is compared
//!   byte-exact; the GPU backend within a tolerance (same-adapter reproducible).
//!
//! It reuses the growth-agent capture plumbing verbatim: neutral render data →
//! `axiom-gpu-backend`'s off-screen arm or `axiom-canvas2d-backend`'s software
//! rasterizer → PNG.

use std::path::Path;
use std::process::ExitCode;

use axiom_canvas2d_backend::Canvas2dBackendApi;
use axiom_gallery::growth::visual_target::abstraction::{self, AbstractionRecord};
use axiom_gallery::growth::visual_target::axes::Scorecard;
use axiom_gallery::growth::visual_target::compare::{self, Tolerance};
use axiom_gallery::growth::visual_target::review::{self, HumanVerdict};
use axiom_gallery::growth::visual_target::target::Target;
use axiom_gallery::growth::visual_target::{build, RenderData};
use axiom_gallery::growth::visual_target::scene::Manifest;
use axiom_gpu_backend::GpuBackendApi;
use axiom_host::{
    FrameCamera, FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport,
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{KernelApi, Ratio};

/// The 4×4 identity matrix (column-major) for the Canvas camera's unused view/proj
/// slots (the backend reads `view_proj` + the per-draw `mvp`).
const IDENTITY_4X4: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// Which backend renders the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Backend {
    Gpu,
    Canvas2d,
}

impl Backend {
    fn parse(s: &str) -> Result<Backend, String> {
        match s {
            "gpu" => Ok(Backend::Gpu),
            "canvas2d" | "canvas" => Ok(Backend::Canvas2d),
            other => Err(format!("unknown backend '{other}' (want gpu | canvas2d)")),
        }
    }

    /// Filename suffix distinguishing a canvas2d render from a gpu one.
    fn suffix(self) -> &'static str {
        match self {
            Backend::Gpu => "",
            Backend::Canvas2d => "_canvas2d",
        }
    }

    /// The default compare tolerance for this backend.
    fn default_tolerance(self) -> Tolerance {
        match self {
            Backend::Gpu => Tolerance::GPU_DEFAULT,
            Backend::Canvas2d => Tolerance::EXACT,
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("");
    let result = match cmd {
        "render" => cmd_render(&args[2..]),
        "bless" => cmd_bless(&args[2..]),
        "compare" => cmd_compare(&args[2..]),
        "status" => cmd_status(&args[2..]),
        "attack" => cmd_attack(&args[2..]),
        "review" => cmd_review(&args[2..]),
        "accept" => cmd_accept(&args[2..]),
        "abstraction" => cmd_abstraction(&args[2..]),
        _ => Err(usage()),
    };
    match result {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("[visual-target] {msg}");
            ExitCode::from(2)
        }
    }
}

fn usage() -> String {
    "usage:\n  \
     -- the deterministic shot --\n  \
     visual-target render  <scene.toml> [--backend gpu|canvas2d] [--out PATH]\n  \
     visual-target bless   <scene.toml> [--backend gpu|canvas2d] [--out PATH]\n  \
     visual-target compare <scene.toml> <reference.png> [--backend ...] [--tol MEAN,MAX] [--diff PATH]\n  \
     -- the convergence review loop (over a target directory) --\n  \
     visual-target status  <target-dir>\n  \
     visual-target attack  <target-dir>\n  \
     visual-target review  <target-dir> [--changed a.toml,b.rs] [--abstraction-introduced]\n  \
     visual-target accept  <target-dir> [--note TEXT]\n  \
     visual-target abstraction <target-dir> --api TEXT --command TEXT --proof TEXT [--inexpressible]"
        .to_string()
}

/// A minimal `--flag value` extractor over the tail args (positionals are the
/// non-flag tokens, in order).
struct Args<'a> {
    positional: Vec<&'a str>,
    flags: Vec<(&'a str, &'a str)>,
}

impl<'a> Args<'a> {
    fn parse(tail: &'a [String]) -> Result<Args<'a>, String> {
        let mut positional = Vec::new();
        let mut flags = Vec::new();
        let mut i = 0;
        while i < tail.len() {
            let tok = tail[i].as_str();
            if let Some(name) = tok.strip_prefix("--") {
                // A `--flag` followed by a non-flag token takes that as its value; a
                // trailing flag or one followed by another `--flag` is a boolean.
                let next = tail.get(i + 1).map(String::as_str);
                let (value, step) = match next {
                    Some(v) if !v.starts_with("--") => (v, 2),
                    _ => ("", 1),
                };
                flags.push((name, value));
                i += step;
            } else {
                positional.push(tok);
                i += 1;
            }
        }
        Ok(Args { positional, flags })
    }

    fn flag(&self, name: &str) -> Option<&'a str> {
        self.flags.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
    }

    /// Whether a boolean flag `--name` is present.
    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(k, _)| *k == name)
    }

    fn backend(&self) -> Result<Backend, String> {
        self.flag("backend").map(Backend::parse).unwrap_or(Ok(Backend::Gpu))
    }
}

/// Load the manifest at `path` and build its neutral render data.
fn load(path: &str) -> Result<RenderData, String> {
    let manifest = Manifest::load(path)?;
    println!(
        "[visual-target] loaded '{}' ({} tree instance(s), {}x{})",
        manifest.name,
        build::all_trees(&manifest).len(),
        manifest.camera.width_px,
        manifest.camera.height_px,
    );
    Ok(build::build(&manifest))
}

fn cmd_render(tail: &[String]) -> Result<ExitCode, String> {
    let args = Args::parse(tail)?;
    let scene = *args.positional.first().ok_or("render needs a <scene.toml> path")?;
    let backend = args.backend()?;
    let rd = load(scene)?;
    let (pixels, w, h) = render(&rd, backend);
    let out = args
        .flag("out")
        .map(str::to_string)
        .unwrap_or_else(|| format!("screenshots/{}{}.png", stem(scene), backend.suffix()));
    write_png(&out, &pixels, w, h)?;
    println!("[visual-target] wrote {out} ({w}x{h}, {backend:?})");
    Ok(ExitCode::SUCCESS)
}

fn cmd_bless(tail: &[String]) -> Result<ExitCode, String> {
    let args = Args::parse(tail)?;
    let scene = *args.positional.first().ok_or("bless needs a <scene.toml> path")?;
    let backend = args.backend()?;
    let rd = load(scene)?;
    let (pixels, w, h) = render(&rd, backend);
    let out = args.flag("out").map(str::to_string).unwrap_or_else(|| reference_path(scene, backend));
    write_png(&out, &pixels, w, h)?;
    println!("[visual-target] blessed reference {out} ({w}x{h}, {backend:?})");
    Ok(ExitCode::SUCCESS)
}

fn cmd_compare(tail: &[String]) -> Result<ExitCode, String> {
    let args = Args::parse(tail)?;
    let scene = *args.positional.first().ok_or("compare needs a <scene.toml> path")?;
    let reference = *args.positional.get(1).ok_or("compare needs a <reference.png> path")?;
    let backend = args.backend()?;
    let tolerance = parse_tolerance(&args, backend)?;

    let rd = load(scene)?;
    let (pixels, w, h) = render(&rd, backend);

    let png = std::fs::read(reference).map_err(|e| format!("read reference {reference}: {e}"))?;
    let (ref_pixels, rw, rh) = compare::decode_rgba_png(&png)?;
    (rw == w && rh == h)
        .then_some(())
        .ok_or_else(|| format!("size mismatch: render {w}x{h}, reference {rw}x{rh}"))?;

    let report = compare::compare_rgba(&pixels, &ref_pixels, w, h, tolerance.per_pixel)?;
    println!(
        "[visual-target] diff: mean={:.3} max={} changed={:.2}%  (tol mean<={:.3} max<={})",
        report.mean_diff,
        report.max_diff,
        report.changed_fraction * 100.0,
        tolerance.mean,
        tolerance.max,
    );

    if let Some(diff_path) = args.flag("diff") {
        let heatmap = compare::diff_heatmap(&pixels, &ref_pixels);
        write_png(diff_path, &heatmap, w, h)?;
        println!("[visual-target] wrote diff heatmap {diff_path}");
    }

    let passed = tolerance.passes(&report);
    println!("[visual-target] {}", if passed { "PASS" } else { "FAIL" });
    Ok(if passed { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

/// Parse an optional `--tol MEAN,MAX` override, defaulting per backend.
fn parse_tolerance(args: &Args, backend: Backend) -> Result<Tolerance, String> {
    let base = backend.default_tolerance();
    match args.flag("tol") {
        None => Ok(base),
        Some(spec) => {
            let (mean, max) = spec.split_once(',').ok_or("--tol wants MEAN,MAX")?;
            let mean = mean.trim().parse::<f32>().map_err(|e| format!("bad --tol mean: {e}"))?;
            let max = max.trim().parse::<u8>().map_err(|e| format!("bad --tol max: {e}"))?;
            Ok(Tolerance { mean, max, per_pixel: base.per_pixel })
        }
    }
}

/// Render the neutral data through the chosen backend to `(rgba, width, height)`.
fn render(rd: &RenderData, backend: Backend) -> (Vec<u8>, u32, u32) {
    match backend {
        Backend::Gpu => (render_gpu(rd), rd.width, rd.height),
        Backend::Canvas2d => render_canvas2d(rd),
    }
}

/// Render through the engine's native off-screen GPU backend (the shadowed, lit,
/// instanced path the browser's WebGPU/WebGL2 arm runs).
fn render_gpu(rd: &RenderData) -> Vec<u8> {
    let mut rgba = GpuBackendApi::render_offscreen_rgba(
        rd.width,
        rd.height,
        &rd.meshes,
        &rd.materials,
        &rd.normals,
        &rd.lights,
        rd.light_view_proj,
        &rd.batches,
        rd.clear,
        None,
        rd.ambient,
        None,
        axiom_host::BackendCapabilityProfile::all(),
        None,
        None,
    )
    .expect("a native GPU adapter renders the visual-target frame");
    // The off-screen GPU path takes raw args (no FramePacket), so the composition
    // applies the SAME backend-neutral whole-frame passes (god-rays, then the filmic
    // tonemap) to its RGBA output that the packet-consuming backends realize internally
    // — these are neutral frame data, identical across every renderer. The GPU always
    // attempts everything (its profile is `all()`), so both passes run when present.
    let lights: Vec<FrameLight> = rd
        .lights
        .iter()
        .map(|(kind, vec, color, intensity)| {
            FrameLight::new(*kind, *vec, [color[0], color[1], color[2], *intensity])
        })
        .collect();
    let camera = Some(FrameCamera::new(IDENTITY_4X4, IDENTITY_4X4, rd.view_proj));
    let mut packet = FramePacket::new(
        0,
        0,
        FrameViewport::new(rd.width, rd.height),
        rd.clear,
        camera,
        Vec::new(),
        lights,
        rd.light_view_proj,
        FrameFeatureSet::new(false, true, 1, 0),
    );
    if let Some(v) = rd.volumetrics {
        packet = packet.with_volumetrics(v);
    }
    if let Some(pp) = rd.postprocess {
        packet = packet.with_postprocess(pp);
    }
    // Both apply passes no-op when the packet carries no such effect; order matters
    // (tonemap grades the composited result, so it runs last).
    axiom_host::apply_frame_volumetrics(&mut rgba, rd.width, rd.height, &packet);
    axiom_host::apply_frame_postprocess(&mut rgba, rd.width, rd.height, &packet);
    rgba
}

/// Render through the software Canvas 2D backend, expanding each instanced batch
/// into per-instance draws (the Canvas packet takes flat draws, not batches).
fn render_canvas2d(rd: &RenderData) -> (Vec<u8>, u32, u32) {
    let request = present_request(rd.width, rd.height);
    let mut backend = Canvas2dBackendApi::new(&request);
    // Per-backend capability config: Canvas 2D uses the profile resolved from the
    // manifest's [canvas2d] section (e.g. skip the god-ray volumetric pass); the GPU
    // path always attempts everything.
    backend.set_capability_profile(rd.canvas2d_profile);
    backend.load_meshes(&rd.meshes);
    backend.set_quality_level(3);

    let mut draws: Vec<FrameDrawItem> = Vec::new();
    let mut object_id = 0u64;
    for (mesh_id, material_id, instances, count) in &rd.batches {
        for i in 0..*count as usize {
            let base = i * 36;
            let mvp = slice16(&instances[base..base + 16]);
            let world = slice16(&instances[base + 16..base + 32]);
            let tint = [
                instances[base + 32],
                instances[base + 33],
                instances[base + 34],
                instances[base + 35],
            ];
            // Only the trees (trunk mesh 2, canopy mesh 3) cast contact shadows;
            // the terrain (1) and the ground-cover tufts (4) do not.
            let casts = *mesh_id == 2 || *mesh_id == 3;
            draws.push(FrameDrawItem::new(object_id, *mesh_id, *material_id, world, mvp, tint, casts));
            object_id += 1;
        }
    }

    let lights: Vec<FrameLight> = rd
        .lights
        .iter()
        .map(|(kind, vec, color, intensity)| {
            FrameLight::new(*kind, *vec, [color[0], color[1], color[2], *intensity])
        })
        .collect();
    let directional = rd.lights.iter().filter(|(k, ..)| *k == 0).count() as u32;
    let point = rd.lights.iter().filter(|(k, ..)| *k == 1).count() as u32;
    let features = FrameFeatureSet::new(false, directional > 0, directional, point);
    let camera = Some(FrameCamera::new(IDENTITY_4X4, IDENTITY_4X4, rd.view_proj));
    let packet = FramePacket::new(
        0,
        0,
        FrameViewport::new(rd.width, rd.height),
        rd.clear,
        camera,
        draws,
        lights,
        rd.light_view_proj,
        features,
    );
    // Neutral volumetric light rides on the packet; the Canvas 2D backend realizes
    // it (via host::apply_frame_volumetrics) exactly as any other backend would.
    let packet = match rd.volumetrics {
        Some(v) => packet.with_volumetrics(v),
        None => packet,
    };
    // Hemisphere ambient rides on the packet too; the Canvas 2D backend lights unlit
    // faces with it, matching the GPU path's ambient uniform.
    let packet = packet.with_ambient(rd.ambient);
    // The filmic tonemap rides on the packet as well; the Canvas 2D backend applies it
    // gated by its capability profile (so the [canvas2d] config can skip it).
    let packet = match rd.postprocess {
        Some(pp) => packet.with_postprocess(pp),
        None => packet,
    };
    backend.render_offscreen_rgba(&packet)
}

fn slice16(s: &[f32]) -> [f32; 16] {
    let mut m = [0.0f32; 16];
    m.copy_from_slice(s);
    m
}

/// The validated host presentation request the Canvas 2D backend is sized from
/// (mirrors the growth-agent bin and `tools/axiom-shot`).
fn present_request(w: u32, h: u32) -> HostPresentationRequest {
    let host = HostApi::new();
    let kernel = KernelApi::new();
    let viewport = host
        .viewport(w, h, Ratio::new(1.0).expect("finite scale"))
        .expect("valid viewport");
    let target = host
        .presentation_target(&kernel, 1, "axiom-visual-target")
        .expect("valid target");
    let surface = host.surface_handle(&kernel, 2).expect("valid surface");
    let descriptor = host.surface_descriptor(
        viewport,
        HostPresentMode::Fifo,
        HostAlphaMode::Opaque,
        HostColorFormat::Bgra8UnormSrgb,
    );
    let adapter = host.adapter_request(HostPowerPreference::HighPerformance, true);
    let device = host.device_request(true, HostDeviceProfile::Baseline);
    host.presentation_request(target, surface, descriptor, adapter, device)
        .expect("valid presentation request")
}

/// Write RGBA8 pixels to a PNG (creating parent dirs), mirroring the growth-agent bin.
fn write_png(path: &str, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let file = std::fs::File::create(path).map_err(|e| format!("create {path}: {e}"))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| format!("PNG header: {e}"))?;
    writer.write_image_data(rgba).map_err(|e| format!("PNG data: {e}"))?;
    Ok(())
}

/// The file stem of a scene path (its name without directory or extension).
fn stem(scene: &str) -> String {
    Path::new(scene)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("visual_target")
        .to_string()
}

/// The default reference PNG path, alongside the scene file.
fn reference_path(scene: &str, backend: Backend) -> String {
    let dir = Path::new(scene).parent().and_then(|p| p.to_str()).unwrap_or(".");
    let dot = match backend {
        Backend::Gpu => String::new(),
        Backend::Canvas2d => ".canvas2d".to_string(),
    };
    format!("{dir}/{}{dot}.reference.png", stem(scene))
}

// ===========================================================================
//  The convergence review loop, operating over a target directory.
// ===========================================================================

/// Print a scorecard as a labelled 0–5 bar chart.
fn print_scorecard(label: &str, card: &Scorecard) {
    println!("[visual-target] {label} scorecard:");
    for (axis, score) in card.scores() {
        let filled = usize::from(score);
        let empty = usize::from(5u8.saturating_sub(score));
        let bar: String =
            std::iter::repeat('#').take(filled).chain(std::iter::repeat('.').take(empty)).collect();
        println!("    {axis:<28} {score}  [{bar}]");
    }
}

/// `status <target-dir>` — champion scores, final score, completion, next flaw.
fn cmd_status(tail: &[String]) -> Result<ExitCode, String> {
    let args = Args::parse(tail)?;
    let dir = *args.positional.first().ok_or("status needs a <target-dir>")?;
    let status = Target::new(dir).status()?;
    print_scorecard("champion", &status.champion);
    println!(
        "[visual-target] final_score {:.3}  (lowest {} * 0.7 + average {:.3} * 0.3)",
        status.final_score,
        status.champion.lowest_score(),
        status.champion.average(),
    );
    println!("[visual-target] iterations so far: {}", status.iterations);
    match status.complete {
        true => println!("[visual-target] COMPLETE — every axis >= 4 or human-accepted"),
        false => println!(
            "[visual-target] next attacked axis: {}",
            status.next_axis.map(|a| a.to_string()).unwrap_or_else(|| "-".to_string()),
        ),
    }
    Ok(ExitCode::SUCCESS)
}

/// `attack <target-dir>` — name the one axis the next candidate must target.
fn cmd_attack(tail: &[String]) -> Result<ExitCode, String> {
    let args = Args::parse(tail)?;
    let dir = *args.positional.first().ok_or("attack needs a <target-dir>")?;
    let status = Target::new(dir).status()?;
    match status.next_axis {
        None => println!("[visual-target] champion is complete — nothing to attack"),
        Some(axis) => {
            println!(
                "[visual-target] ATTACK axis: {axis}  (champion score {})",
                status.champion.get(axis)
            );
            println!(
                "[visual-target] make ONE bounded change in manifest.candidate.toml aimed at '{axis}',"
            );
            println!(
                "[visual-target] render candidate.png, author scorecard.candidate.toml, then `review`."
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `review <target-dir> [--changed a,b] [--abstraction-introduced]` — decide,
/// append a ledger entry, promote on a win, emit diagnostics.
fn cmd_review(tail: &[String]) -> Result<ExitCode, String> {
    let args = Args::parse(tail)?;
    let dir = *args.positional.first().ok_or("review needs a <target-dir>")?;
    let target = Target::new(dir);
    let changed: Vec<String> = args
        .flag("changed")
        .map(|s| s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect())
        .unwrap_or_else(|| vec!["manifest.candidate.toml".to_string()]);
    let outcome = target.review(changed, args.has("abstraction-introduced"))?;
    let entry = &outcome.entry;
    println!("[visual-target] iteration {}  attacked {}", entry.iteration, outcome.attacked_axis);
    let override_note = outcome.resolved.human_overrode.then_some(" (human override)").unwrap_or("");
    println!("[visual-target] decision: {}{override_note}", outcome.resolved.effective);
    println!("[visual-target] reason: {}", entry.reason);
    match outcome.promoted {
        true => println!("[visual-target] champion REPLACED by candidate"),
        false => println!("[visual-target] champion kept"),
    }
    println!(
        "[visual-target] next attacked axis: {}",
        entry.next_attacked_axis.map(|a| a.to_string()).unwrap_or_else(|| "COMPLETE".to_string()),
    );
    for d in &outcome.diagnostics {
        println!("[visual-target] diagnostic: diagnostics/{d}");
    }
    println!("[visual-target] ledger: {}", target.ledger_path().display());
    Ok(ExitCode::SUCCESS)
}

/// `accept <target-dir> [--note TEXT]` — record a human acceptance of the champion
/// as complete (the human half of the completion criterion).
fn cmd_accept(tail: &[String]) -> Result<ExitCode, String> {
    let args = Args::parse(tail)?;
    let dir = *args.positional.first().ok_or("accept needs a <target-dir>")?;
    let target = Target::new(dir);
    let note = args.flag("note").unwrap_or("champion accepted by human");
    let verdict =
        HumanVerdict { accept_champion: true, note: note.to_string(), ..Default::default() };
    let toml = toml::to_string_pretty(&verdict).map_err(|e| format!("serialize verdict: {e}"))?;
    std::fs::write(target.verdict_path(), toml)
        .map_err(|e| format!("write {}: {e}", target.verdict_path().display()))?;
    println!("[visual-target] wrote {} (accept_champion = true)", target.verdict_path().display());
    println!("[visual-target] champion accepted as complete: {note}");
    Ok(ExitCode::SUCCESS)
}

/// `abstraction <target-dir> --api T --command T --proof T [--inexpressible]` —
/// ask the gate whether an abstraction may be introduced for the currently attacked
/// axis, and if so write the justified record. Exits non-zero (3) if forbidden.
fn cmd_abstraction(tail: &[String]) -> Result<ExitCode, String> {
    let args = Args::parse(tail)?;
    let dir = *args.positional.first().ok_or("abstraction needs a <target-dir>")?;
    let target = Target::new(dir);
    let champion = target.champion_scorecard()?;
    let verdict = target.verdict()?;
    let axis = review::next_attacked_axis(&champion, verdict.as_ref())
        .ok_or("champion is complete; no axis to unlock an abstraction for")?;
    let ledger = target.ledger()?;
    let permission = abstraction::permit(&ledger, axis, args.has("inexpressible"));
    println!("[visual-target] abstraction gate for {axis}: {}", permission.describe());
    match permission.is_permitted() {
        false => Ok(ExitCode::from(3)),
        true => {
            let api = args.flag("api").ok_or("abstraction needs --api TEXT")?;
            let command = args.flag("command").ok_or("abstraction needs --command TEXT")?;
            let proof = args.flag("proof").ok_or("abstraction needs --proof TEXT")?;
            let record = AbstractionRecord::new(&permission, api, command, proof)?;
            std::fs::create_dir_all(target.abstractions_dir())
                .map_err(|e| format!("create abstractions dir: {e}"))?;
            let existing = std::fs::read_dir(target.abstractions_dir())
                .map(|rd| {
                    rd.filter_map(Result::ok)
                        .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
                        .count()
                })
                .unwrap_or(0);
            let path = target.abstractions_dir().join(format!("{:04}.toml", existing + 1));
            std::fs::write(&path, record.to_toml())
                .map_err(|e| format!("write {}: {e}", path.display()))?;
            println!("[visual-target] wrote abstraction record {}", path.display());
            Ok(ExitCode::SUCCESS)
        }
    }
}
