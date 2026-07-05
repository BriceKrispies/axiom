//! Behavioral tests for the semantic slice checks (`check-slices` and
//! `check-slice-placement`). Each test paves a small synthetic slice under a
//! temp root, runs the checker, and asserts the exact [`ViolationKind`] fires on
//! a broken slice and is absent on a healthy one — behavior, not shape.

use std::path::{Path, PathBuf};

use xtask::slice_check::{check_slice_placement, check_slices, hex_sha256};
use xtask::violation::{CheckReport, ViolationKind};

/// A synthetic workspace root paved into a unique temp directory.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("axiom_xtask_slice_fx_{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    fn write(&self, rel: &str, contents: &[u8]) -> &Self {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
        self
    }

    /// Pave a slice crate under `apps/<dir>/` with `slice.toml`, a `lib.rs`
    /// exporting `harness_entry`, and a determinism test file.
    fn slice(&self, dir: &str, slice_toml: &str, harness_entry: &str, det_test: &str) -> &Self {
        self.write(
            &format!("apps/{dir}/slice.toml"),
            slice_toml.as_bytes(),
        )
        .write(
            &format!("apps/{dir}/src/lib.rs"),
            format!("pub fn {harness_entry}() {{}}\n").as_bytes(),
        )
        .write(
            &format!("apps/{dir}/tests/{det_test}.rs"),
            b"#[test] fn t() { assert!(true); }\n",
        )
    }

    fn report(&self) -> CheckReport {
        let mut report = CheckReport::default();
        check_slices(&self.root, &mut report);
        report.finish()
    }

    fn placement_report(&self) -> CheckReport {
        let mut report = CheckReport::default();
        check_slice_placement(&self.root, &mut report);
        report.finish()
    }
}

/// A slice.toml body with one golden. `sha` is the recorded hash.
fn manifest(golden_sha: &str, harness: Option<&str>) -> String {
    let harness_line = harness
        .map(|h| format!("harness = \"{h}\"\n"))
        .unwrap_or_default();
    format!(
        "[slice]\n\
         name = \"demo\"\n\
         crate_name = \"axiom-demo\"\n\
         harness_entry = \"build_demo\"\n\
         determinism_test = \"det\"\n\
         {harness_line}\n\
         [[golden]]\n\
         path = \"tests/golden/g.bin\"\n\
         sha256 = \"{golden_sha}\"\n"
    )
}

const GOLDEN_BYTES: &[u8] = b"golden-artifact-bytes";

#[test]
fn a_healthy_headless_slice_passes() {
    let f = Fixture::new("healthy");
    let sha = hex_sha256(GOLDEN_BYTES);
    f.slice("axiom-demo", &manifest(&sha, None), "build_demo", "det")
        .write("apps/axiom-demo/tests/golden/g.bin", GOLDEN_BYTES);
    let report = f.report();
    assert!(report.is_ok(), "expected clean, got: {:?}", report.violations());
}

#[test]
fn missing_determinism_test_fires() {
    let f = Fixture::new("no_det");
    let sha = hex_sha256(GOLDEN_BYTES);
    // Pave everything but the determinism test file.
    f.write("apps/axiom-demo/slice.toml", manifest(&sha, None).as_bytes())
        .write("apps/axiom-demo/src/lib.rs", b"pub fn build_demo() {}\n")
        .write("apps/axiom-demo/tests/golden/g.bin", GOLDEN_BYTES);
    assert!(f.report().has_kind(ViolationKind::SliceDeterminismTestMissing));
}

#[test]
fn missing_golden_file_fires() {
    let f = Fixture::new("no_golden");
    let sha = hex_sha256(GOLDEN_BYTES);
    f.slice("axiom-demo", &manifest(&sha, None), "build_demo", "det");
    // No g.bin written.
    assert!(f.report().has_kind(ViolationKind::SliceGoldenMissing));
}

#[test]
fn golden_hash_mismatch_fires() {
    let f = Fixture::new("bad_hash");
    // Record a hash that does NOT match the committed bytes.
    let wrong = hex_sha256(b"different-bytes");
    f.slice("axiom-demo", &manifest(&wrong, None), "build_demo", "det")
        .write("apps/axiom-demo/tests/golden/g.bin", GOLDEN_BYTES);
    let report = f.report();
    assert!(report.has_kind(ViolationKind::SliceGoldenHashMismatch));
    assert!(!report.has_kind(ViolationKind::SliceGoldenMissing));
}

#[test]
fn reference_hash_mismatch_and_missing_fire() {
    let mismatch = Fixture::new("ref_mismatch");
    let sha = hex_sha256(GOLDEN_BYTES);
    let with_ref = format!(
        "{}\n[reference]\npath = \"reference/r.png\"\nsha256 = \"{}\"\n",
        manifest(&sha, None),
        hex_sha256(b"expected-image")
    );
    mismatch
        .slice("axiom-demo", &with_ref, "build_demo", "det")
        .write("apps/axiom-demo/tests/golden/g.bin", GOLDEN_BYTES)
        .write("apps/axiom-demo/reference/r.png", b"actual-different-image");
    assert!(mismatch
        .report()
        .has_kind(ViolationKind::SliceReferenceHashMismatch));

    let missing = Fixture::new("ref_missing");
    missing
        .slice("axiom-demo", &with_ref, "build_demo", "det")
        .write("apps/axiom-demo/tests/golden/g.bin", GOLDEN_BYTES);
    // reference/r.png not written.
    assert!(missing.report().has_kind(ViolationKind::SliceReferenceMissing));
}

#[test]
fn harness_entry_not_exported_fires() {
    let f = Fixture::new("no_entry");
    let sha = hex_sha256(GOLDEN_BYTES);
    // Manifest declares harness_entry = build_demo, but lib.rs exports something else.
    f.write("apps/axiom-demo/slice.toml", manifest(&sha, None).as_bytes())
        .write("apps/axiom-demo/src/lib.rs", b"pub fn other_symbol() {}\n")
        .write("apps/axiom-demo/tests/det.rs", b"#[test] fn t(){assert!(true);}\n")
        .write("apps/axiom-demo/tests/golden/g.bin", GOLDEN_BYTES);
    assert!(f.report().has_kind(ViolationKind::SliceHarnessEntryMissing));
}

#[test]
fn declared_harness_not_registered_fires() {
    let f = Fixture::new("unregistered");
    let sha = hex_sha256(GOLDEN_BYTES);
    // Declares a live harness, but there is no axiom-shot registry that names it.
    f.slice("axiom-demo", &manifest(&sha, Some("demo-live")), "build_demo", "det")
        .write("apps/axiom-demo/tests/golden/g.bin", GOLDEN_BYTES);
    assert!(f
        .report()
        .has_kind(ViolationKind::SliceHarnessNotRegistered));
}

#[test]
fn declared_harness_registered_in_shot_passes() {
    let f = Fixture::new("registered");
    let sha = hex_sha256(GOLDEN_BYTES);
    f.slice("axiom-demo", &manifest(&sha, Some("demo-live")), "build_demo", "det")
        .write("apps/axiom-demo/tests/golden/g.bin", GOLDEN_BYTES)
        // A registry source that registers the name as a string literal.
        .write(
            "tools/axiom-shot/src/registry.rs",
            b"pub fn registry() { let _ = SliceEntry { name: \"demo-live\" }; }\n",
        );
    let report = f.report();
    assert!(
        !report.has_kind(ViolationKind::SliceHarnessNotRegistered),
        "registered harness must pass, got: {:?}",
        report.violations()
    );
}

#[test]
fn invalid_slice_manifest_fires() {
    let f = Fixture::new("invalid");
    f.write(
        "apps/axiom-demo/slice.toml",
        b"[slice]\nname = \"demo\"\nmystery = true\n",
    );
    assert!(f.report().has_kind(ViolationKind::SliceManifestInvalid));
}

#[test]
fn placement_flags_a_large_hidden_engine_transform() {
    let f = Fixture::new("placement_bad");
    let body = std::iter::repeat("    let m = Mat4::default(); let v = Vec3::ZERO; // mesh instance world")
        .take(400)
        .collect::<Vec<_>>()
        .join("\n");
    let src = format!("pub fn build_geometry() -> Vec3 {{\n{body}\n}}\n");
    f.write("apps/axiom-demo/src/build.rs", src.as_bytes());
    assert!(f
        .placement_report()
        .has_kind(ViolationKind::SlicePlacementEngineLogicInApp));
}

#[test]
fn placement_ignores_small_glue_and_real_composition() {
    let f = Fixture::new("placement_ok");
    // Small glue file.
    f.write(
        "apps/axiom-demo/src/glue.rs",
        b"pub fn wire() { let r = RenderApi::new(); }\n",
    );
    // A large file that genuinely composes several module facades.
    let body = std::iter::repeat("    let m = Mat4::default(); // mesh instance world")
        .take(400)
        .collect::<Vec<_>>()
        .join("\n");
    let src = format!(
        "pub fn compose() {{\n\
         let a = SceneApi::new(); let b = RenderApi::new(); let c = ResourcesApi::new();\n\
         {body}\n}}\n"
    );
    f.write("apps/axiom-demo/src/compose.rs", src.as_bytes());
    assert!(f.placement_report().is_ok());
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("repo root is two levels above crates/xtask")
}

/// The real repository's seeded slices (rotating-cube, retro-fps) must satisfy
/// the slice contract. This wires `check-slices` into `cargo test --workspace`
/// so a broken/regenerated golden or an un-harnessable slice fails CI.
#[test]
fn real_repo_slices_pass() {
    let mut report = CheckReport::default();
    check_slices(&repo_root(), &mut report);
    let report = report.finish();
    assert!(
        report.is_ok(),
        "the real repo violates the slice contract: {:?}",
        report.violations()
    );
}

// Reference to `Path` so an unused-import lint never fires if a helper is
// commented out during local development.
#[allow(dead_code)]
fn _path_ref(_: &Path) {}
