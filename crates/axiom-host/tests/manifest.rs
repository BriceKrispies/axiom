//! LayerManifest tests for axiom-host (Layer 03).

use axiom_kernel::{KernelApi, KernelErrorCode, LayerCapability, LayerDependency, LayerManifest};

const HOST_INDEX: u16 = 3;
const KERNEL_INDEX: u16 = 0;
const RUNTIME_INDEX: u16 = 1;
const MATH_INDEX: u16 = 2;

// Layer-03 logical capabilities — stable u32 codes per documented capability.
const CAP_HOST_BOUNDARY_CONTRACTS: u32 = 1;
const CAP_VIEWPORT_VALIDATION: u32 = 2;
const CAP_EXPLICIT_HOST_FRAME_INPUT: u32 = 3;
const CAP_LIFECYCLE_SIGNALS: u32 = 4;
const CAP_DETERMINISTIC_HOST_STEP_PLANNING: u32 = 5;
const CAP_RUNTIME_HOST_STEP_DRIVER: u32 = 6;
const CAP_HOST_FRAME_REPORTING: u32 = 7;
const CAP_HOST_ERROR_PROPAGATION: u32 = 8;

fn host_manifest_with_full_contract() -> LayerManifest {
    let api = KernelApi::new();
    api.layer_manifest(HOST_INDEX, "axiom-host")
        .with_dependency(LayerDependency::new(KERNEL_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(RUNTIME_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(MATH_INDEX))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_HOST_BOUNDARY_CONTRACTS))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_VIEWPORT_VALIDATION))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_EXPLICIT_HOST_FRAME_INPUT))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_LIFECYCLE_SIGNALS))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_DETERMINISTIC_HOST_STEP_PLANNING))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_RUNTIME_HOST_STEP_DRIVER))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_HOST_FRAME_REPORTING))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_HOST_ERROR_PROPAGATION))
        .unwrap()
}

#[test]
fn host_manifest_validates() {
    let manifest = host_manifest_with_full_contract();
    assert_eq!(manifest.index(), HOST_INDEX);
    assert_eq!(manifest.name(), "axiom-host");
    assert_eq!(manifest.dependencies().len(), 3);
    assert_eq!(manifest.capabilities().len(), 8);
    assert!(manifest.validate().is_ok());
}

#[test]
fn host_may_import_layers_zero_one_and_two() {
    let api = KernelApi::new();
    let manifest = api
        .layer_manifest(HOST_INDEX, "axiom-host")
        .with_dependency(LayerDependency::new(KERNEL_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(RUNTIME_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(MATH_INDEX))
        .unwrap();
    assert!(manifest.validate().is_ok());
}

#[test]
fn host_must_be_able_to_import_math_specifically() {
    // Layer 02 (math) is the host's immediate previous layer. The host
    // manifest must accept it as a dependency for validation to pass.
    let api = KernelApi::new();
    let manifest = api
        .layer_manifest(HOST_INDEX, "axiom-host")
        .with_dependency(LayerDependency::new(MATH_INDEX))
        .unwrap();
    assert!(manifest.validate().is_ok());
}

#[test]
fn host_may_not_import_itself() {
    let api = KernelApi::new();
    let manifest = api
        .layer_manifest(HOST_INDEX, "axiom-host")
        .with_dependency(LayerDependency::new(HOST_INDEX))
        .unwrap();
    assert_eq!(
        manifest.validate().unwrap_err().code(),
        KernelErrorCode::SelfImport
    );
}

#[test]
fn host_may_not_import_future_layers() {
    let api = KernelApi::new();
    for future in [4u16, 5, 6, 7] {
        let manifest = api
            .layer_manifest(HOST_INDEX, "axiom-host")
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
        .layer_manifest(HOST_INDEX, "axiom-host")
        .with_dependency(LayerDependency::new(MATH_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(MATH_INDEX))
        .unwrap_err();
    assert_eq!(err.code(), KernelErrorCode::DuplicateDependency);
}

#[test]
fn duplicate_capability_is_rejected() {
    let api = KernelApi::new();
    let err = api
        .layer_manifest(HOST_INDEX, "axiom-host")
        .with_capability(LayerCapability::new(CAP_VIEWPORT_VALIDATION))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_VIEWPORT_VALIDATION))
        .unwrap_err();
    assert_eq!(err.code(), KernelErrorCode::DuplicateCapability);
}
