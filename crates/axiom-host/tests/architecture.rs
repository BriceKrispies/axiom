//! Mechanical architecture enforcement for axiom-host (an Axiom layer).

use std::fs;
use std::path::{Path, PathBuf};

fn host_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn sibling_src_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(name)
        .join("src")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("src directory must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn host_source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs(&host_src_dir(), &mut files);
    assert!(!files.is_empty(), "expected host source files");
    files.sort();
    files
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("source must be valid UTF-8")
}

/// Strip `//` line comments and string-literal contents so a forbidden token
/// appearing only inside documentation or a string literal cannot trip the
/// scan.
fn strip_comments_and_strings(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut in_char = false;
    while let Some(c) = chars.next() {
        if in_string {
            if c == '\\' {
                chars.next();
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            continue;
        }
        if in_char {
            if c == '\\' {
                chars.next();
                continue;
            }
            if c == '\'' {
                in_char = false;
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            continue;
        }
        if c == '\'' {
            in_char = true;
            continue;
        }
        out.push(c);
    }
    out
}

fn assert_absent_in(dir: &Path, label: &str, forbidden: &[&str], why: &str) {
    let mut files = Vec::new();
    collect_rs(dir, &mut files);
    files.sort();
    let mut violations = Vec::new();
    for path in &files {
        let stripped = strip_comments_and_strings(&read(path));
        for needle in forbidden {
            if stripped.contains(needle) {
                violations.push(format!(
                    "{label} {}: contains forbidden `{needle}`",
                    path.display()
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}

fn assert_absent(forbidden: &[&str], why: &str) {
    assert_absent_in(&host_src_dir(), "axiom-host", forbidden, why);
}

/// Like `assert_absent`, but treats each needle as a *whole type-path token*: it
/// flags a match only when the needle is NOT immediately followed by an
/// identifier-continuation character. This bans a renderer type named exactly
/// `Texture` (`foo::Texture`, `foo::Texture)`, `foo::Texture,`) while allowing
/// the host-owned `TextureId` 2D-draw handle (`foo::TextureId`) — a distinct
/// value-type whose `Id` suffix the naive substring scan would false-match.
fn assert_absent_type_tokens(forbidden: &[&str], why: &str) {
    let mut files = Vec::new();
    collect_rs(&host_src_dir(), &mut files);
    files.sort();
    let mut violations = Vec::new();
    for path in &files {
        let stripped = strip_comments_and_strings(&read(path));
        for needle in forbidden {
            let mut rest = stripped.as_str();
            while let Some(pos) = rest.find(needle) {
                let after = &rest[pos + needle.len()..];
                let next_is_ident = after
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
                if !next_is_ident {
                    violations.push(format!(
                        "axiom-host {}: contains forbidden type token `{needle}`",
                        path.display()
                    ));
                }
                rest = &rest[pos + needle.len()..];
            }
        }
    }
    assert!(violations.is_empty(), "{why}\n{}", violations.join("\n"));
}

#[test]
fn no_browser_or_js_bindgen_apis() {
    assert_absent(
        &["web_sys", "js_sys", "wasm_bindgen", "wasm-bindgen"],
        "axiom-host must not reference browser / JS bindings",
    );
}

#[test]
fn no_dom_canvas_or_browser_globals() {
    assert_absent(
        &[
            "HtmlCanvas",
            "HtmlElement",
            "OffscreenCanvas",
            "document.",
            "window.",
            "navigator.",
        ],
        "axiom-host must not reference DOM/canvas/browser globals",
    );
}

#[test]
fn no_webgpu_or_webgl_apis() {
    assert_absent(
        &["wgpu", "webgpu", "WebGpu", "WebGL", "webgl", "GPUDevice"],
        "axiom-host must not reference WebGPU/WebGL",
    );
}

#[test]
fn no_animation_frame_or_performance_now() {
    assert_absent(
        &["requestAnimationFrame", "performance.now"],
        "axiom-host must not call browser frame/clock APIs",
    );
}

#[test]
fn no_wall_clock_time() {
    assert_absent(
        &["std::time", "SystemTime", "Instant::now", "chrono"],
        "axiom-host must not read wall-clock time",
    );
}

#[test]
fn no_randomness() {
    assert_absent(
        &["rand::", "thread_rng", "random()", "fastrand", "getrandom"],
        "axiom-host must not use randomness",
    );
}

#[test]
fn no_console_printing() {
    assert_absent(
        &["println!", "eprintln!", "print!", "eprint!", "dbg!"],
        "axiom-host must emit structured records, not print",
    );
}

#[test]
fn no_placeholder_macros() {
    assert_absent(
        &["todo!", "unimplemented!"],
        "axiom-host must contain no placeholder architecture",
    );
}

#[test]
fn no_global_mutable_state() {
    assert_absent(
        &["static mut", "lazy_static"],
        "axiom-host must not use global mutable state",
    );
}

#[test]
fn no_renderer_or_shader_concepts() {
    // Whole-type-token scan: `::Texture` is banned as a type name, but the
    // host-owned `TextureId` 2D-draw handle (relocated from axiom-draw2d) is
    // allowed — its `Id` suffix makes it a distinct value type, not a renderer
    // resource. Renderer GPU types are independently banned by the wgpu/WebGPU
    // hygiene tests and the import-allowlist test.
    assert_absent_type_tokens(
        &[
            "::Renderer",
            "::RenderPipeline",
            "::Shader",
            "::ShaderModule",
            "::Material",
            "::Mesh",
            "::Texture",
            "::Swapchain",
        ],
        "axiom-host must not absorb renderer / shader / material concepts",
    );
}

#[test]
fn no_higher_engine_layer_concepts() {
    assert_absent(
        &[
            "::World",
            "::Scene",
            "::SceneGraph",
            "::Asset",
            "::AssetLoader",
            "::Physics",
            "::RigidBody",
            "::Collider",
            "::Animation",
            "::Animator",
            "::Audio",
            "::SoundSource",
            "::InputState",
            "::KeyCode",
            "::MouseButton",
            "::Gamepad",
            "::Plugin",
            "::EditorPanel",
            "::GameLoop",
            "rapier",
            "wgpu",
            "winit",
            "egui",
            "bevy",
        ],
        "axiom-host must not import a layer it does not declare in depends_on",
    );
}

#[test]
fn no_utils_or_helpers_modules() {
    for path in host_source_files() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        assert_ne!(name, "utils", "axiom-host must not have a `utils` module");
        assert_ne!(
            name, "helpers",
            "axiom-host must not have a `helpers` module"
        );
        assert_ne!(name, "common", "axiom-host must not have a `common` module");
        assert_ne!(name, "misc", "axiom-host must not have a `misc` module");
    }
}

#[test]
fn lib_exports_are_curated_set() {
    let lib = read(&host_src_dir().join("lib.rs"));
    let mut actual: Vec<&str> = lib
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") && !line.starts_with("pub(crate)"))
        .collect();
    actual.sort();

    let mut expected: Vec<&str> = vec![
        "pub use host_api::HostApi;",
        "pub use host_boundary_config::HostBoundaryConfig;",
        "pub use host_error::HostError;",
        "pub use host_error_code::HostErrorCode;",
        "pub use host_frame_input::HostFrameInput;",
        "pub use host_frame_report::HostFrameReport;",
        "pub use host_lifecycle_signal::HostLifecycleSignal;",
        "pub use host_lifecycle_state::HostLifecycleState;",
        "pub use host_orientation::Orientation;",
        "pub use host_result::HostResult;",
        "pub use host_safe_area_insets::HostSafeAreaInsets;",
        // A request to bake one procedural surface on the GPU. The neutral
        // contract between the app that authors it and the backend that runs
        // it — Module Law #8 forbids the backend publishing its own.
        "pub use procedural_bake::BakeOutput;",
        "pub use procedural_bake::ProceduralBakeMaps;",
        "pub use procedural_bake::ProceduralBakeRequest;",
        "pub use pixels::Pixels;",
        "pub use render_scale::RenderScale;",
        "pub use render_scale::RenderScaleController;",
        "pub use host_skip_reason::HostSkipReason;",
        "pub use host_step_driver::HostStepDriver;",
        "pub use host_step_plan::HostStepPlan;",
        "pub use host_viewport::HostViewport;",
        "pub use host_metrics::HostMetrics;",
        "pub use host_outcome::HostOutcome;",
        "pub use host_outcome_set::HostOutcomeSet;",
        "pub use host_param_value::HostParamValue;",
        "pub use host_session_config::HostSessionConfig;",
        "pub use host_session_params::HostSessionParams;",
        "pub use player_id::PlayerId;",
        "pub use score::Score;",
        "pub use host_adapter_request::HostAdapterRequest;",
        "pub use host_alpha_mode::HostAlphaMode;",
        // The off-screen attachment vocabulary, sibling to the surface colour
        // format: HDR/float/depth targets a multi-pass frame graph names, which a
        // window surface can never present in.
        "pub use host_attachment_format::HostAttachmentFormat;",
        "pub use host_color_format::HostColorFormat;",
        "pub use host_device_profile::HostDeviceProfile;",
        "pub use host_device_request::HostDeviceRequest;",
        "pub use host_present_mode::HostPresentMode;",
        "pub use host_presentation_report::HostPresentationReport;",
        "pub use host_presentation_request::HostPresentationRequest;",
        "pub use host_presentation_status::HostPresentationStatus;",
        "pub use host_presentation_target::HostPresentationTarget;",
        "pub use host_power_preference::HostPowerPreference;",
        "pub use host_surface_descriptor::HostSurfaceDescriptor;",
        "pub use host_surface_handle::HostSurfaceHandle;",
        "pub use frame_ambient::FrameAmbient;",
        "pub use frame_bloom::FrameBloom;",
        "pub use frame_bloom::rolloff_knee;",
        "pub use frame_bloom::luminance;",
        "pub use frame_capability::BackendCapabilityProfile;",
        "pub use frame_capability::CapabilityDegradation;",
        "pub use frame_capability::RenderCapability;",
        // The frame's atmospheric depth fog — the neutral aerial-perspective
        // contract both backends read (added with the GPU fog term; the curated
        // set must name every export, so it is pinned here like its `FrameAmbient`
        // sibling).
        "pub use frame_depth_fog::FrameDepthFog;",
        "pub use frame_indirect::FrameIndirect;",
        "pub use frame_packet::FrameCamera;",
        "pub use frame_packet::FrameDrawItem;",
        "pub use frame_packet::FrameFeatureSet;",
        "pub use frame_packet::FrameLight;",
        "pub use frame_packet::FramePacket;",
        "pub use frame_packet::FrameViewport;",
        // The redraw ledger: "can this frame differ from the one already on the
        // screen?", answered against the packet itself so no input can be
        // forgotten, plus the revision naming what the packet cannot carry.
        "pub use frame_packet::FrameRevision;",
        "pub use frame_packet::PresentationLedger;",
        "pub use frame_packet::RedrawDecision;",
        "pub use frame_packet::RedrawVerdict;",
        "pub use frame_postprocess::apply_frame_postprocess;",
        "pub use frame_postprocess::FramePostProcess;",
        // The app-authored render look (ambient + fog + sky + bloom), bound as
        // one value so a new look knob does not widen four signatures and a
        // dozen wasm-only call sites.
        "pub use frame_render_look::FrameRenderLook;",
        "pub use frame_sky::FrameSky;",
        "pub use frame_retro_32bit::apply_frame_retro_32bit;",
        "pub use frame_retro_32bit::FrameRetro32BitProfile;",
        "pub use frame_volumetrics::apply_frame_volumetrics;",
        "pub use frame_volumetrics::FrameVolumetrics;",
        // The app's authored tone map. Also the switch for the whole HDR path:
        // a scene target is only `Rgba16Float` when an app has authored one AND
        // the device granted `HdrTargets`, so this is what decides whether a
        // value above 1.0 survives the scene pass at all.
        "pub use frame_tonemap::FrameTonemap;",
        // A material's albedo pixels plus how they must be filtered. Bind-time
        // resident state rather than frame-packet data, and the one texture
        // property a backend cannot derive from the payload itself.
        "pub use material_texture::MaterialTexture;",
        "pub use material_texture::TextureSampling;",
        // One non-albedo map's extent + RGBA8 texels — the payload behind each of
        // the carrier's four optional map slots (normal, ORM+height, detail,
        // macro). Exported because the producer of those slots lives in another
        // crate (`axiom`'s `RunningApp::material_textures`) and must be able to
        // name what it builds.
        "pub use material_texture::MapPixels;",
        "pub use frame_raster_stats::FrameDepthCueStats;",
        "pub use frame_raster_stats::FrameRasterStats;",
        "pub use frame_submission_report::BackendKind;",
        "pub use frame_submission_report::FrameFeature;",
        "pub use frame_submission_report::FrameSubmissionReport;",
        "pub use sdf_scene::SdfPrimitive;",
        "pub use sdf_scene::SdfScene;",
        // Backend-neutral 2D draw contract (SPEC-04), relocated from axiom-draw2d.
        "pub use camera2d::Camera2d;",
        "pub use common2d::Common2d;",
        "pub use common2d::Shadow2d;",
        "pub use draw2d_command::Draw2dCommand;",
        "pub use draw2d_list::Draw2dList;",
        "pub use fill2d::Fill2d;",
        "pub use fill2d::Stroke2d;",
        "pub use handles::FontHandle;",
        "pub use handles::PaintId;",
        "pub use handles::RenderTargetId;",
        "pub use handles::TextureId;",
        "pub use handles::TransformDepth;",
        "pub use paint::GradientStop;",
        "pub use rect::Rect;",
        "pub use rgba::Rgba;",
        "pub use sprite_draw2d::SpriteDraw2d;",
        "pub use text2d::Glyph2d;",
        "pub use text2d::GlyphRun;",
        "pub use text2d::TextAlign;",
        "pub use text2d::TextDraw2d;",
        "pub use text2d::TextMetrics;",
    ];
    expected.sort();

    assert_eq!(
        actual, expected,
        "axiom-host's lib.rs public exports must match the curated set; \
         update both lib.rs and this test together"
    );
}

#[test]
fn axiom_kernel_does_not_import_axiom_host() {
    assert_absent_in(
        &sibling_src_dir("axiom-kernel"),
        "axiom-kernel",
        &["axiom_host", "axiom-host"],
        "axiom-kernel must not import axiom-host",
    );
}

#[test]
fn axiom_runtime_does_not_import_axiom_host() {
    assert_absent_in(
        &sibling_src_dir("axiom-runtime"),
        "axiom-runtime",
        &["axiom_host", "axiom-host"],
        "axiom-runtime must not import axiom-host",
    );
}

#[test]
fn axiom_math_does_not_import_axiom_host() {
    assert_absent_in(
        &sibling_src_dir("axiom-math"),
        "axiom-math",
        &["axiom_host", "axiom-host"],
        "axiom-math must not import axiom-host",
    );
}

#[test]
fn host_only_imports_declared_dependencies() {
    let mut illegal = Vec::new();
    for path in host_source_files() {
        let stripped = strip_comments_and_strings(&read(&path));
        for line in stripped.lines() {
            let trimmed = line.trim();
            if !trimmed.contains("axiom_") {
                continue;
            }
            for chunk in trimmed.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if chunk.starts_with("axiom_")
                    && chunk != "axiom_kernel"
                    && chunk != "axiom_runtime"
                    && chunk != "axiom_math"
                    && chunk != "axiom_host"
                {
                    illegal.push(format!("{}: {}", path.display(), trimmed));
                }
            }
        }
    }
    assert!(
        illegal.is_empty(),
        "axiom-host may only import axiom-kernel, axiom-runtime, and axiom-math:\n{}",
        illegal.join("\n")
    );
}
