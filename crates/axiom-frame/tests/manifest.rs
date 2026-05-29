//! LayerManifest tests for axiom-frame (Layer 04).

use axiom_kernel::{KernelApi, KernelErrorCode, LayerCapability, LayerDependency, LayerManifest};

const FRAME_INDEX: u16 = 4;
const KERNEL_INDEX: u16 = 0;
const RUNTIME_INDEX: u16 = 1;
const MATH_INDEX: u16 = 2;
const HOST_INDEX: u16 = 3;

// Layer-04 logical capabilities — stable u32 codes per documented capability.
const CAP_CANONICAL_ENGINE_FRAME: u32 = 1;
const CAP_FRAME_CONTEXT: u32 = 2;
const CAP_FRAME_STEP_SUMMARY: u32 = 3;
const CAP_FRAME_TIMING_SUMMARY: u32 = 4;
const CAP_FRAME_VIEWPORT_SNAPSHOT: u32 = 5;
const CAP_FRAME_LIFECYCLE_STATE: u32 = 6;
const CAP_DETERMINISTIC_FRAME_COMMAND_QUEUE: u32 = 7;
const CAP_FRAME_DIAGNOSTICS: u32 = 8;
const CAP_HOST_FRAME_ADAPTATION: u32 = 9;

fn frame_manifest_with_full_contract() -> LayerManifest {
    let api = KernelApi::new();
    api.layer_manifest(FRAME_INDEX, "axiom-frame")
        .with_dependency(LayerDependency::new(KERNEL_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(RUNTIME_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(MATH_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(HOST_INDEX))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_CANONICAL_ENGINE_FRAME))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_FRAME_CONTEXT))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_FRAME_STEP_SUMMARY))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_FRAME_TIMING_SUMMARY))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_FRAME_VIEWPORT_SNAPSHOT))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_FRAME_LIFECYCLE_STATE))
        .unwrap()
        .with_capability(LayerCapability::new(
            CAP_DETERMINISTIC_FRAME_COMMAND_QUEUE,
        ))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_FRAME_DIAGNOSTICS))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_HOST_FRAME_ADAPTATION))
        .unwrap()
}

#[test]
fn frame_manifest_validates() {
    let manifest = frame_manifest_with_full_contract();
    assert_eq!(manifest.index(), FRAME_INDEX);
    assert_eq!(manifest.name(), "axiom-frame");
    assert_eq!(manifest.dependencies().len(), 4);
    assert_eq!(manifest.capabilities().len(), 9);
    assert!(manifest.validate().is_ok());
}

#[test]
fn frame_may_import_layers_zero_one_two_and_three() {
    let api = KernelApi::new();
    let manifest = api
        .layer_manifest(FRAME_INDEX, "axiom-frame")
        .with_dependency(LayerDependency::new(KERNEL_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(RUNTIME_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(MATH_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(HOST_INDEX))
        .unwrap();
    assert!(manifest.validate().is_ok());
}

#[test]
fn frame_must_be_able_to_import_host_specifically() {
    // Layer 03 (host) is the frame's immediate previous layer.
    let api = KernelApi::new();
    let manifest = api
        .layer_manifest(FRAME_INDEX, "axiom-frame")
        .with_dependency(LayerDependency::new(HOST_INDEX))
        .unwrap();
    assert!(manifest.validate().is_ok());
}

#[test]
fn frame_may_not_import_itself() {
    let api = KernelApi::new();
    let manifest = api
        .layer_manifest(FRAME_INDEX, "axiom-frame")
        .with_dependency(LayerDependency::new(FRAME_INDEX))
        .unwrap();
    assert_eq!(
        manifest.validate().unwrap_err().code(),
        KernelErrorCode::SelfImport
    );
}

#[test]
fn frame_may_not_import_future_layers() {
    let api = KernelApi::new();
    for future in [5u16, 6, 7, 8] {
        let manifest = api
            .layer_manifest(FRAME_INDEX, "axiom-frame")
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
        .layer_manifest(FRAME_INDEX, "axiom-frame")
        .with_dependency(LayerDependency::new(HOST_INDEX))
        .unwrap()
        .with_dependency(LayerDependency::new(HOST_INDEX))
        .unwrap_err();
    assert_eq!(err.code(), KernelErrorCode::DuplicateDependency);
}

#[test]
fn duplicate_capability_is_rejected() {
    let api = KernelApi::new();
    let err = api
        .layer_manifest(FRAME_INDEX, "axiom-frame")
        .with_capability(LayerCapability::new(CAP_FRAME_CONTEXT))
        .unwrap()
        .with_capability(LayerCapability::new(CAP_FRAME_CONTEXT))
        .unwrap_err();
    assert_eq!(err.code(), KernelErrorCode::DuplicateCapability);
}
