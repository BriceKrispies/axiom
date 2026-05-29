//! Behavioral tests for the architecture checker, driven by the fixtures under
//! `tests/fixtures/`. Each failing fixture isolates one rule and asserts the
//! specific `ViolationKind` is reported; the valid fixtures must pass cleanly.

use std::path::PathBuf;

use xtask::check::check_architecture;
use xtask::violation::ViolationKind;

fn fixture(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(case)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("repo root is two levels above crates/xtask")
}

#[test]
fn case_01_valid_chain_passes() {
    let report = check_architecture(&fixture("01_valid_chain"));
    assert!(
        report.is_ok(),
        "expected a clean report, got: {:?}",
        report.violations()
    );
    assert_eq!(report.layers_checked, vec!["kernel", "runtime"]);
}

#[test]
fn case_02_future_import_fails() {
    let report = check_architecture(&fixture("02_future_import"));
    assert!(!report.is_ok());
    assert!(
        report.has_kind(ViolationKind::FutureImport),
        "expected FutureImport, got: {:?}",
        report.violations()
    );
}

#[test]
fn case_03_missing_previous_import_fails() {
    let report = check_architecture(&fixture("03_missing_prev_import"));
    assert!(!report.is_ok());
    assert!(
        report.has_kind(ViolationKind::MissingPreviousImport),
        "expected MissingPreviousImport, got: {:?}",
        report.violations()
    );
}

#[test]
fn case_04_private_path_import_fails() {
    let report = check_architecture(&fixture("04_private_path"));
    assert!(!report.is_ok());
    assert!(
        report.has_kind(ViolationKind::PrivatePathImport),
        "expected PrivatePathImport, got: {:?}",
        report.violations()
    );
}

#[test]
fn case_05_missing_proof_export_fails() {
    let report = check_architecture(&fixture("05_missing_proof_export"));
    assert!(!report.is_ok());
    assert!(
        report.has_kind(ViolationKind::MissingProofExport),
        "expected MissingProofExport, got: {:?}",
        report.violations()
    );
}

#[test]
fn case_06_proof_missing_reference_fails() {
    let report = check_architecture(&fixture("06_proof_missing_reference"));
    assert!(!report.is_ok());
    assert!(
        report.has_kind(ViolationKind::ProofReferenceMissing),
        "expected ProofReferenceMissing, got: {:?}",
        report.violations()
    );
}

#[test]
fn case_07_valid_proof_passes() {
    let report = check_architecture(&fixture("07_valid_proof"));
    assert!(
        report.is_ok(),
        "expected a clean report, got: {:?}",
        report.violations()
    );
    assert_eq!(report.layers_checked, vec!["kernel", "runtime"]);
}

/// The real repository must satisfy the Layer Law. This wires the checker into
/// `cargo test --workspace` so architecture regressions fail the test suite.
#[test]
fn real_repo_layers_pass() {
    let report = check_architecture(&repo_root());
    assert!(
        report.is_ok(),
        "the real Axiom layers violate the Layer Law: {:?}",
        report.violations()
    );
    assert!(
        report.layers_checked.contains(&"kernel".to_string()),
        "expected the kernel layer to be discovered; checked: {:?}",
        report.layers_checked
    );
}
