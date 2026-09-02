//! **Every generator composes into valid WGSL — proved without a GPU.**
//!
//! The eighteen `.wgsl` files beside `materials::wgsl` are *fragments*. None of
//! them is a compilable unit: a surface body references the bake header's
//! uniform block `U` and the library's `owFbm01`/`owSRGB`, and the library's own
//! three pieces reference each other (`noise.wgsl` calls `owMod2`, which
//! `gl_semantics.wgsl` declares). Opened alone, each one fails to parse:
//!
//! ```text
//! concrete.wgsl      no definition in scope for identifier: `U`
//! rust_helpers.wgsl  no definition in scope for identifier: `owSRGB`
//! noise.wgsl         no definition in scope for identifier: `owMod2`
//! ```
//!
//! That is not a defect to fix by making each file whole. The prelude every
//! generator needs is 11,400 bytes (header 496 + library 10,424 + footer 480)
//! against 79,729 bytes of actual bodies, so eighteen self-contained files
//! would be **72% duplicated boilerplate** with the noise library copied
//! eighteen times — the exact shape the Atlas Friction Law calls a repo defect.
//! The unit that is complete is the *program*, and the backend is what composes
//! it.
//!
//! # Why this test exists next to `materials_gpu_bake_port`
//!
//! `every_generator_compiles_and_bakes` already proves these compile — on a
//! real device, through `--features offscreen`. That is the most expensive
//! proof available and it cannot run where there is no adapter, which is most
//! CI. So a typo in a `.wgsl` file was, until now, only catchable on a machine
//! with a GPU.
//!
//! This splits the two claims apart:
//!
//! * **validity** — a `naga` parse and validate of the composed program. No
//!   device, no feature flag, no `wgpu`; the whole suite runs in milliseconds.
//! * **correctness** — the CPU/GPU parity bake, which stays where it is.
//!
//! `naga` is the same parser `wgpu` uses internally, at the version `wgpu`
//! pins, so "parses here" and "compiles there" are the same question.
//!
//! # The side effect is deliberate
//!
//! Each composed program is written to `target/wgsl-programs/<name>.wgsl`.
//! That directory is the only place a complete, editable, LSP-readable version
//! of these shaders exists — the checked-in fragments cannot be one — and
//! writing them is also what makes a failure debuggable: `naga`'s error points
//! at a line of the *composed* program, and this is the file that has those
//! line numbers.

use axiom_gpu_backend::GpuBackendApi;
use axiom_shmup::materials::wgsl;

/// Where the composed, complete programs land. Under `target/`, so it is
/// already gitignored: these are derived artifacts, and checking them in would
/// re-create the duplication the fragments exist to avoid.
const OUT_DIR: &str = "../../target/wgsl-programs";

/// Composes one generator's whole program, exactly as the backend will.
fn program_for(generator: &str) -> String {
    GpuBackendApi::bake_program_wgsl(&wgsl::library_wgsl(), wgsl::generator_wgsl(generator))
}

/// Parses and validates, returning the rendered diagnostic on failure.
fn validate(source: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| format!("parse:\n{}", e.emit_to_string(source)))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map(|_| ())
    .map_err(|e| format!("validate: {e:?}"))
}

/// **The gate.** Every generator named by the dispatch table composes into a
/// program that `naga` accepts.
///
/// One test over all eighteen rather than eighteen tests: a transcription
/// mistake usually lands in the shared library, and eighteen simultaneous
/// failures reporting one root cause is noise. The failures are collected and
/// reported together, with the generator named.
#[test]
fn every_generator_composes_into_valid_wgsl() {
    std::fs::create_dir_all(OUT_DIR).expect("the output directory can be created");

    let failures: Vec<String> = wgsl::GENERATOR_NAMES
        .iter()
        .filter_map(|name| {
            let program = program_for(name);
            std::fs::write(format!("{OUT_DIR}/{name}.wgsl"), &program)
                .expect("the composed program can be written");
            validate(&program).err().map(|why| format!("{name}: {why}"))
        })
        .collect();

    assert!(
        failures.is_empty(),
        "{} of {} generator(s) do not compose into valid WGSL. The composed \
         programs are in `{OUT_DIR}` and the line numbers below are theirs.\n\n{}",
        failures.len(),
        wgsl::GENERATOR_NAMES.len(),
        failures.join("\n\n")
    );
}

/// The fragments are **not** compilable units, and that is a fact worth pinning
/// rather than a limitation to discover twice.
///
/// If this ever starts passing, something changed the composition contract —
/// most likely a generator stopped needing the header or the library, which
/// would mean the splice is no longer doing what `bake_program_wgsl` documents.
/// Either way it is a finding, not an improvement.
#[test]
fn a_bare_generator_body_is_not_a_valid_program_on_its_own() {
    let bare = wgsl::generator_wgsl("concrete");
    let err = validate(bare).expect_err("a surface body alone cannot resolve `U`");
    assert!(
        err.contains('U'),
        "expected the missing uniform block to be what fails, got:\n{err}"
    );
}

/// The library's three pieces only resolve **concatenated, in order** — which
/// is the whole reason `library_wgsl` exists rather than three separate uses.
///
/// The asymmetry with a generator body is worth stating, because it is not
/// obvious and it bounds how much of the fragment problem is fixable: WGSL does
/// not require an entry point, so once concatenated the library **is** a valid
/// module on its own. Only the surface bodies are irreducibly incomplete, since
/// they reach for the bake header's `U`. If the three library files were ever
/// merged into one, that one file would be LSP-clean; the eighteen generators
/// could not be, at any arrangement short of duplicating the prelude into each.
#[test]
fn the_library_only_resolves_concatenated_in_order() {
    validate(wgsl::noise::NOISE)
        .expect_err("`noise` alone calls `owMod2`, which `gl_semantics` declares");
    validate(wgsl::metal::RUST_HELPERS)
        .expect_err("`rust_helpers` alone calls `owSRGB`, which `noise` declares");

    validate(&wgsl::library_wgsl())
        .expect("concatenated in `library_wgsl`'s order, every call resolves");
}
