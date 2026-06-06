//! Workspace-package classification: every workspace member must be one of
//! [`PackageClass::Layer`], [`PackageClass::Module`], [`PackageClass::App`],
//! or [`PackageClass::Tool`]. An unclassified workspace package is an
//! architecture error.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::app_manifest::AppManifest;
use crate::cargo_metadata::WorkspacePackage;
use crate::manifest::LayerManifest;
use crate::module_manifest::ModuleManifest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PackageClass {
    Layer,
    Module,
    App,
    Tool,
    /// A build-time support crate (today: the `axiom-zones` zone-marker
    /// proc-macros). It is not on the runtime spine, every layer/module/app may
    /// depend on it, and it is excluded from the coverage gate.
    Support,
}

#[derive(Debug, Clone)]
pub struct Classified {
    pub package: WorkspacePackage,
    pub class: PackageClass,
}

/// Bag of all parsed manifests by package directory.
#[derive(Debug, Default)]
pub struct ManifestIndex {
    pub layer_by_dir: BTreeMap<PathBuf, LayerManifest>,
    pub module_by_dir: BTreeMap<PathBuf, ModuleManifest>,
    pub app_by_dir: BTreeMap<PathBuf, AppManifest>,
}

impl ManifestIndex {
    pub fn new(
        layers: &[LayerManifest],
        modules: &[ModuleManifest],
        apps: &[AppManifest],
    ) -> Self {
        let mut idx = ManifestIndex::default();
        for m in layers {
            idx.layer_by_dir.insert(m.dir.clone(), m.clone());
        }
        for m in modules {
            idx.module_by_dir.insert(m.dir.clone(), m.clone());
        }
        for m in apps {
            idx.app_by_dir.insert(m.dir.clone(), m.clone());
        }
        idx
    }
}

/// Classify a workspace package using the rules:
/// - `crates/<name>/layer.toml`           → Layer
/// - `modules/<name>/module.toml`         → Module
/// - `apps/<name>/app.toml`               → App
/// - package name `"xtask"`               → Tool
/// - package name `"axiom-zones"`         → Support
/// - `tools/<name>/...`                   → Tool
/// - anything else                        → `None`
pub fn classify(
    root: &Path,
    pkg: &WorkspacePackage,
    index: &ManifestIndex,
) -> Option<PackageClass> {
    if index.layer_by_dir.contains_key(&pkg.dir) {
        return Some(PackageClass::Layer);
    }
    if index.module_by_dir.contains_key(&pkg.dir) {
        return Some(PackageClass::Module);
    }
    if index.app_by_dir.contains_key(&pkg.dir) {
        return Some(PackageClass::App);
    }
    if pkg.name == "xtask" {
        return Some(PackageClass::Tool);
    }
    // The zone-marker proc-macro crate is build-time engine support, not a layer.
    if pkg.name == "axiom-zones" {
        return Some(PackageClass::Support);
    }
    if let Ok(rel) = pkg.dir.strip_prefix(root) {
        let first = rel.components().next().and_then(|c| c.as_os_str().to_str());
        if first == Some("tools") {
            return Some(PackageClass::Tool);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_dir_is_classified_as_layer() {
        let pkg = WorkspacePackage {
            name: "axiom-kernel".into(),
            dir: PathBuf::from("/repo/crates/axiom-kernel"),
            workspace_deps: vec![],
        };
        let layer_manifest = LayerManifest {
            dir: pkg.dir.clone(),
            layer: crate::manifest::LayerSection {
                name: "kernel".into(),
                index: 0,
                previous: None,
                crate_name: Some("axiom-kernel".into()),
                allowed_dependencies: vec![],
                forbidden_dependencies: vec![],
                meaningful_dependency: "Base layer.".into(),
                introduced_capabilities: vec![],
                consumed_capabilities: vec![],
            },
            proof_exports: vec![],
        };
        let index = ManifestIndex::new(&[layer_manifest], &[], &[]);
        assert_eq!(
            classify(Path::new("/repo"), &pkg, &index),
            Some(PackageClass::Layer)
        );
    }

    #[test]
    fn xtask_package_is_a_tool() {
        let pkg = WorkspacePackage {
            name: "xtask".into(),
            dir: PathBuf::from("/repo/crates/xtask"),
            workspace_deps: vec![],
        };
        let index = ManifestIndex::default();
        assert_eq!(
            classify(Path::new("/repo"), &pkg, &index),
            Some(PackageClass::Tool)
        );
    }

    #[test]
    fn axiom_zones_package_is_support() {
        let pkg = WorkspacePackage {
            name: "axiom-zones".into(),
            dir: PathBuf::from("/repo/crates/axiom-zones"),
            workspace_deps: vec![],
        };
        let index = ManifestIndex::default();
        assert_eq!(
            classify(Path::new("/repo"), &pkg, &index),
            Some(PackageClass::Support)
        );
    }

    #[test]
    fn package_under_tools_dir_is_a_tool() {
        let pkg = WorkspacePackage {
            name: "fmt-check".into(),
            dir: PathBuf::from("/repo/tools/fmt-check"),
            workspace_deps: vec![],
        };
        let index = ManifestIndex::default();
        assert_eq!(
            classify(Path::new("/repo"), &pkg, &index),
            Some(PackageClass::Tool)
        );
    }

    #[test]
    fn package_with_no_manifest_in_unknown_dir_is_unclassified() {
        let pkg = WorkspacePackage {
            name: "stray".into(),
            dir: PathBuf::from("/repo/somewhere/stray"),
            workspace_deps: vec![],
        };
        let index = ManifestIndex::default();
        assert_eq!(classify(Path::new("/repo"), &pkg, &index), None);
    }
}
