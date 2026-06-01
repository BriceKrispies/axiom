//! The `module.toml` manifest schema and loader.
//!
//! One manifest lives in each module crate at `modules/<module>/module.toml`.
//! Modules are *isolated capabilities*: they may depend on a curated set of
//! layers but never on another module, an app, or a tool. The schema is
//! deliberately small and rejects unknown fields so future agents must
//! extend it explicitly.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// A parsed `module.toml`, paired with the directory it was found in.
#[derive(Debug, Clone)]
pub struct ModuleManifest {
    /// The crate directory containing this manifest (e.g. `modules/scene`).
    pub dir: PathBuf,
    pub module: ModuleSection,
}

impl ModuleManifest {
    /// The Rust import prefix other crates use to import this module
    /// (e.g. `axiom_scene`). Derived from `crate_name` by replacing `-`
    /// with `_`.
    pub fn import_prefix(&self) -> String {
        self.module.crate_name.replace('-', "_")
    }

    /// The module's source directory (`<dir>/src`).
    pub fn src_dir(&self) -> PathBuf {
        self.dir.join("src")
    }
}

/// The `[module]` table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleSection {
    /// Short logical module name (e.g. `"scene"`). Must be unique among
    /// modules in the workspace.
    pub name: String,
    /// The actual cargo package name (e.g. `"axiom-scene"`). Must match
    /// the workspace package this manifest belongs to.
    pub crate_name: String,
    /// Kind tag. `"feature-module"` marks a *composition* module that may
    /// depend on a curated set of other modules (declared in
    /// `allowed_modules`); any other value (or absent) is an isolated
    /// *engine module* whose `allowed_modules` must be empty.
    #[serde(default)]
    pub kind: Option<String>,
    /// Logical layer names this module is permitted to depend on. Every
    /// name must resolve to an existing layer.
    #[serde(default)]
    pub allowed_layers: Vec<String>,
    /// Required, **must be empty** today. Reserved for a future, explicit
    /// exception system; until then modules never depend on other modules.
    #[serde(default)]
    pub allowed_modules: Vec<String>,
    /// Logical capability names this module publishes. Duplicates are
    /// rejected at load time.
    #[serde(default)]
    pub introduced_capabilities: Vec<String>,
}

impl ModuleSection {
    /// Whether this is a feature (composition) module — one permitted to
    /// depend on the modules listed in `allowed_modules`.
    pub fn is_feature_module(&self) -> bool {
        self.kind.as_deref() == Some("feature-module")
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    module: ModuleSection,
}

/// An error loading or parsing a module manifest.
#[derive(Debug)]
pub struct ModuleManifestError {
    pub path: PathBuf,
    pub message: String,
}

/// Parse a single module manifest's text.
pub fn parse_module_manifest(
    dir: &Path,
    text: &str,
) -> Result<ModuleManifest, ModuleManifestError> {
    let raw: RawManifest = toml::from_str(text).map_err(|e| ModuleManifestError {
        path: dir.join("module.toml"),
        message: e.message().to_string(),
    })?;
    let manifest = ModuleManifest {
        dir: dir.to_path_buf(),
        module: raw.module,
    };
    validate_local(&manifest).map_err(|message| ModuleManifestError {
        path: dir.join("module.toml"),
        message,
    })?;
    Ok(manifest)
}

/// Validation that requires only the manifest itself (no cross-manifest
/// context).
fn validate_local(m: &ModuleManifest) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for cap in &m.module.introduced_capabilities {
        if !seen.insert(cap.as_str()) {
            return Err(format!(
                "module `{}` introduces duplicate capability `{cap}`",
                m.module.name
            ));
        }
    }
    Ok(())
}

/// Discover and parse every module manifest at `<root>/modules/*/module.toml`.
///
/// Discovery is one level deep, mirroring the layer manifest loader.
pub fn load_module_manifests(
    root: &Path,
) -> (Vec<ModuleManifest>, Vec<ModuleManifestError>) {
    let modules_dir = root.join("modules");
    let mut manifests = Vec::new();
    let mut errors = Vec::new();

    let entries = match std::fs::read_dir(&modules_dir) {
        Ok(entries) => entries,
        Err(_) => return (manifests, errors),
    };

    let mut crate_dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    crate_dirs.sort();

    for crate_dir in crate_dirs {
        let manifest_path = crate_dir.join("module.toml");
        if !manifest_path.is_file() {
            continue;
        }
        match std::fs::read_to_string(&manifest_path) {
            Ok(text) => match parse_module_manifest(&crate_dir, &text) {
                Ok(manifest) => manifests.push(manifest),
                Err(err) => errors.push(err),
            },
            Err(e) => errors.push(ModuleManifestError {
                path: manifest_path,
                message: format!("could not read file: {e}"),
            }),
        }
    }

    (manifests, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_a_full_module_manifest() {
        let text = r#"
            [module]
            name = "scene"
            crate_name = "axiom-scene"
            kind = "engine-module"
            allowed_layers = ["kernel", "runtime", "math", "frame"]
            allowed_modules = []
            introduced_capabilities = ["scene-graph", "transform-hierarchy"]
        "#;
        let m = parse_module_manifest(Path::new("modules/scene"), text).unwrap();
        assert_eq!(m.module.name, "scene");
        assert_eq!(m.module.crate_name, "axiom-scene");
        assert_eq!(m.import_prefix(), "axiom_scene");
        assert_eq!(m.module.allowed_layers.len(), 4);
        assert!(m.module.allowed_modules.is_empty());
    }

    #[test]
    fn duplicate_capability_is_rejected() {
        let text = r#"
            [module]
            name = "scene"
            crate_name = "axiom-scene"
            allowed_layers = ["kernel"]
            allowed_modules = []
            introduced_capabilities = ["scene-graph", "scene-graph"]
        "#;
        let err = parse_module_manifest(Path::new("modules/scene"), text).unwrap_err();
        assert!(err.message.contains("duplicate capability"));
    }

    #[test]
    fn unknown_field_is_rejected() {
        let text = r#"
            [module]
            name = "scene"
            crate_name = "axiom-scene"
            allowed_layers = []
            allowed_modules = []
            mystery = true
        "#;
        assert!(parse_module_manifest(Path::new("modules/scene"), text).is_err());
    }

    #[test]
    fn minimal_manifest_parses() {
        let text = r#"
            [module]
            name = "scene"
            crate_name = "axiom-scene"
        "#;
        let m = parse_module_manifest(Path::new("modules/scene"), text).unwrap();
        assert!(m.module.allowed_layers.is_empty());
        assert!(m.module.allowed_modules.is_empty());
    }
}
