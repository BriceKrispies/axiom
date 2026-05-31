//! A layer's declared identity, dependencies and capabilities — with validation.

use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::layer_capability::LayerCapability;
use crate::layer_dependency::LayerDependency;
use crate::layer_import_rule::LayerImportRule;
use crate::result::KernelResult;

/// The declared contract of a single layer.
///
/// A manifest names a layer (by index and static name) and lists what it
/// depends on and what it provides. Duplicates are rejected at the moment they
/// are added, and [`Self::validate`] enforces the import rules:
/// - the kernel (index `0`) must declare no dependencies, and
/// - every dependency must satisfy [`LayerImportRule`] (no self / forward import).
///
/// [`Self::kernel`] returns the canonical Layer 00 manifest: index `0`, named
/// `"axiom-kernel"`, with no dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerManifest {
    index: u16,
    name: &'static str,
    dependencies: Vec<LayerDependency>,
    capabilities: Vec<LayerCapability>,
}

impl LayerManifest {
    /// Start a manifest for the layer at `index` named `name`, with no
    /// dependencies or capabilities yet.
    pub fn new(index: u16, name: &'static str) -> Self {
        LayerManifest {
            index,
            name,
            dependencies: Vec::new(),
            capabilities: Vec::new(),
        }
    }

    /// The canonical kernel (Layer 00) manifest.
    pub fn kernel() -> Self {
        LayerManifest::new(0, "axiom-kernel")
    }

    /// Add a dependency, rejecting an exact duplicate.
    ///
    /// Returns [`KernelErrorCode::DuplicateDependency`] if already present.
    pub fn with_dependency(mut self, dependency: LayerDependency) -> KernelResult<Self> {
        if self.dependencies.contains(&dependency) {
            return Err(KernelError::new(
                KernelErrorScope::Layer,
                KernelErrorCode::DuplicateDependency,
                "layer declared the same dependency more than once",
            ));
        }
        self.dependencies.push(dependency);
        Ok(self)
    }

    /// Add a capability, rejecting an exact duplicate.
    ///
    /// Returns [`KernelErrorCode::DuplicateCapability`] if already present.
    pub fn with_capability(mut self, capability: LayerCapability) -> KernelResult<Self> {
        if self.capabilities.contains(&capability) {
            return Err(KernelError::new(
                KernelErrorScope::Layer,
                KernelErrorCode::DuplicateCapability,
                "layer declared the same capability more than once",
            ));
        }
        self.capabilities.push(capability);
        Ok(self)
    }

    /// The layer's index.
    pub fn index(&self) -> u16 {
        self.index
    }

    /// The layer's static name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The declared dependencies, in declaration order.
    pub fn dependencies(&self) -> &[LayerDependency] {
        &self.dependencies
    }

    /// The declared capabilities, in declaration order.
    pub fn capabilities(&self) -> &[LayerCapability] {
        &self.capabilities
    }

    /// Validate the manifest against the architecture import rules.
    ///
    /// The kernel must import nothing; every other layer's dependencies must
    /// target strictly lower indices.
    pub fn validate(&self) -> KernelResult<()> {
        if self.index == 0 && !self.dependencies.is_empty() {
            return Err(KernelError::new(
                KernelErrorScope::Layer,
                KernelErrorCode::KernelMustNotImport,
                "the kernel layer (index 0) must declare no dependencies",
            ));
        }
        for dependency in &self.dependencies {
            LayerImportRule::validate(self.index, dependency.layer())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_manifest_is_layer_zero_with_no_imports() {
        let kernel = LayerManifest::kernel();
        assert_eq!(kernel.index(), 0);
        assert_eq!(kernel.name(), "axiom-kernel");
        assert!(kernel.dependencies().is_empty());
        assert!(kernel.validate().is_ok());
    }

    #[test]
    fn kernel_with_a_dependency_fails_validation() {
        // A manifest at index 0 that declares any dependency must be rejected
        // specifically as KernelMustNotImport, before the generic import rule.
        let manifest = LayerManifest::new(0, "axiom-kernel")
            .with_dependency(LayerDependency::new(7))
            .unwrap();
        assert_eq!(
            manifest.validate().unwrap_err().code(),
            KernelErrorCode::KernelMustNotImport
        );
    }

    #[test]
    fn valid_previous_layer_import_succeeds() {
        let manifest = LayerManifest::new(2, "axiom-fake")
            .with_dependency(LayerDependency::new(0))
            .unwrap()
            .with_dependency(LayerDependency::new(1))
            .unwrap();
        assert!(manifest.validate().is_ok());
        assert_eq!(manifest.dependencies().len(), 2);
    }

    #[test]
    fn self_import_fails_validation() {
        let manifest = LayerManifest::new(3, "axiom-fake")
            .with_dependency(LayerDependency::new(3))
            .unwrap();
        assert_eq!(
            manifest.validate().unwrap_err().code(),
            KernelErrorCode::SelfImport
        );
    }

    #[test]
    fn forward_import_fails_validation() {
        let manifest = LayerManifest::new(1, "axiom-fake")
            .with_dependency(LayerDependency::new(5))
            .unwrap();
        assert_eq!(
            manifest.validate().unwrap_err().code(),
            KernelErrorCode::ForwardImport
        );
    }

    #[test]
    fn duplicate_dependency_is_rejected() {
        let err = LayerManifest::new(2, "axiom-fake")
            .with_dependency(LayerDependency::new(0))
            .unwrap()
            .with_dependency(LayerDependency::new(0))
            .unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::DuplicateDependency);
    }

    #[test]
    fn duplicate_capability_is_rejected() {
        let err = LayerManifest::new(2, "axiom-fake")
            .with_capability(LayerCapability::new(10))
            .unwrap()
            .with_capability(LayerCapability::new(10))
            .unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::DuplicateCapability);
    }

    #[test]
    fn index_reports_the_constructed_nonzero_value() {
        // Distinguishes `index -> 0`: a non-kernel layer reports its real index.
        let manifest = LayerManifest::new(7, "axiom-fake");
        assert_eq!(manifest.index(), 7);
    }

    #[test]
    fn distinct_capabilities_are_kept_in_order() {
        let manifest = LayerManifest::new(1, "axiom-fake")
            .with_capability(LayerCapability::new(1))
            .unwrap()
            .with_capability(LayerCapability::new(2))
            .unwrap();
        assert_eq!(
            manifest.capabilities(),
            &[LayerCapability::new(1), LayerCapability::new(2)]
        );
    }
}

#[cfg(test)]
mod cov {
    use super::*;

    #[test]
    fn validate_covers_index_and_dependency_branches() {
        assert!(LayerManifest::kernel().validate().is_ok()); // index 0, no deps
        let kernel_with_dep = LayerManifest::new(0, "k")
            .with_dependency(LayerDependency::new(3))
            .unwrap();
        assert!(kernel_with_dep.validate().is_err()); // index 0 + deps
        let normal = LayerManifest::new(2, "l")
            .with_dependency(LayerDependency::new(1))
            .unwrap();
        assert!(normal.validate().is_ok()); // index != 0
    }
}
