//! LayerManifest tests for axiom-math (Layer 02).
//!
//! These prove that the kernel's deterministic manifest model accepts the
//! Layer-02 contract: index 2, imports {kernel, runtime}, eight documented
//! capabilities, and rejects duplicates / forward imports / self imports.

use axiom_kernel::{KernelApi, KernelErrorCode, LayerCapability, LayerDependency, LayerManifest};

// Symbolic IDs that mirror `layer.toml` — kept in sync by these tests.
const MATH_INDEX: u16 = 2;
const KERNEL_INDEX: u16 = 0;
const RUNTIME_INDEX: u16 = 1;

// Layer-02 logical capabilities. Each documented capability is identified by
// a stable `u32` code; the manifest model rejects duplicates.
const CAP_SCALAR_POLICY: u32 = 1;
const CAP_VECTOR_MATH: u32 = 2;
const CAP_QUAT_MATH: u32 = 3;
const CAP_MATRIX_MATH: u32 = 4;
const CAP_TRANSFORM_COMPOSITION: u32 = 5;
const CAP_GEOMETRY_PRIMITIVES: u32 = 6;
const CAP_MATH_SERIALIZATION: u32 = 7;
const CAP_CHECKED_MATH_ERRORS: u32 = 8;

fn math_manifest_with_full_contract() -> LayerManifest {
    let api = KernelApi::new();
    api.layer_manifest(MATH_INDEX, "axiom-math")
        .with_dependency(LayerDependency::new(KERNEL_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(RUNTIME_INDEX))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_SCALAR_POLICY))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_VECTOR_MATH))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_QUAT_MATH))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_MATRIX_MATH))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_TRANSFORM_COMPOSITION))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_GEOMETRY_PRIMITIVES))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_MATH_SERIALIZATION))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_CHECKED_MATH_ERRORS))
        .unwrap()
}

#[test]
fn math_manifest_validates() {
    let manifest = math_manifest_with_full_contract();
    assert_eq!(manifest.index(), MATH_INDEX);
    assert_eq!(manifest.name(), "axiom-math");
    assert_eq!(manifest.dependencies().len(), 2);
    assert_eq!(manifest.capabilities().len(), 8);
    assert!(manifest.validate().is_ok());
}

#[test]
fn math_may_import_layers_zero_and_one() {
    let api = KernelApi::new();
    let manifest = api
        .layer_manifest(MATH_INDEX, "axiom-math")
        .with_dependency(LayerDependency::new(KERNEL_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(RUNTIME_INDEX))
        .unwrap();
    assert!(manifest.validate().is_ok());
}

#[test]
fn math_may_not_import_itself() {
    let api = KernelApi::new();
    let manifest = api
        .layer_manifest(MATH_INDEX, "axiom-math")
        .with_dependency(LayerDependency::new(MATH_INDEX))
        .unwrap();
    assert_eq!(
        manifest.validate().unwrap_err().code(),
        KernelErrorCode::SelfImport
    );
}

#[test]
fn math_may_not_import_future_layers() {
    let api = KernelApi::new();
    // Layers 3, 4, 5 do not exist yet; importing any one of them must fail
    // as a forward import.
    for future in [3u16, 4, 5] {
        let manifest = api
            .layer_manifest(MATH_INDEX, "axiom-math")
            .with_dependency(LayerDependency::new(future))
            .unwrap();
        assert_eq!(
            manifest.validate().unwrap_err().code(),
            KernelErrorCode::ForwardImport,
            "depending on future layer {future} must fail as a forward import"
        );
    }
}

#[test]
fn duplicate_dependency_is_rejected() {
    let api = KernelApi::new();
    let err = api
        .layer_manifest(MATH_INDEX, "axiom-math")
        .with_dependency(LayerDependency::new(KERNEL_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(KERNEL_INDEX))
        .unwrap_err();
    assert_eq!(err.code(), KernelErrorCode::DuplicateDependency);
}

#[test]
fn duplicate_capability_is_rejected() {
    let api = KernelApi::new();
    let err = api
        .layer_manifest(MATH_INDEX, "axiom-math")
        .with_capability(LayerCapability::new(CAP_VECTOR_MATH))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_VECTOR_MATH))
        .unwrap_err();
    assert_eq!(err.code(), KernelErrorCode::DuplicateCapability);
}
