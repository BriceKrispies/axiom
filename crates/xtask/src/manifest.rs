//! The `layer.toml` manifest schema and loader.
//!
//! One manifest lives in each layer crate at `crates/<layer>/layer.toml`. The
//! schema is intentionally small and derive-based so future agents can extend it
//! by adding a field to a struct here.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// A parsed `layer.toml`, paired with the directory it was found in.
#[derive(Debug, Clone)]
pub struct LayerManifest {
    /// The crate directory containing this manifest (e.g. `crates/axiom-kernel`).
    pub dir: PathBuf,
    pub layer: LayerSection,
    pub proof_exports: Vec<ProofExport>,
}

impl LayerManifest {
    /// The Rust import prefix other layers use to import this one.
    ///
    /// Derived from `crate_name` (default `axiom-<name>`) by replacing `-` with
    /// `_`, matching how cargo maps crate names to import identifiers.
    pub fn import_prefix(&self) -> String {
        let crate_name = self
            .layer
            .crate_name
            .clone()
            .unwrap_or_else(|| format!("axiom-{}", self.layer.name));
        crate_name.replace('-', "_")
    }

    /// The layer's source directory (`<dir>/src`).
    pub fn src_dir(&self) -> PathBuf {
        self.dir.join("src")
    }
}

/// The `[layer]` table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerSection {
    pub name: String,
    pub index: u32,
    #[serde(default)]
    pub previous: Option<String>,
    #[serde(default)]
    pub crate_name: Option<String>,
    #[serde(default)]
    pub allowed_dependencies: Vec<String>,
    #[serde(default)]
    pub forbidden_dependencies: Vec<String>,
    pub meaningful_dependency: String,
    #[serde(default)]
    pub introduced_capabilities: Vec<String>,
    #[serde(default)]
    pub consumed_capabilities: Vec<String>,
}

/// One `[[proof_exports]]` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProofExport {
    pub export: String,
    #[serde(default)]
    pub must_reference: Vec<String>,
}

/// The raw TOML shape, before we attach the directory.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    layer: LayerSection,
    #[serde(default)]
    proof_exports: Vec<ProofExport>,
}

/// An error loading or parsing a manifest. Carries the offending path so the
/// checker can surface a precise message.
#[derive(Debug)]
pub struct ManifestError {
    pub path: PathBuf,
    pub message: String,
}

/// Parse a single manifest's text.
pub fn parse_manifest(dir: &Path, text: &str) -> Result<LayerManifest, ManifestError> {
    let raw: RawManifest = toml::from_str(text).map_err(|e| ManifestError {
        path: dir.join("layer.toml"),
        message: e.message().to_string(),
    })?;
    Ok(LayerManifest {
        dir: dir.to_path_buf(),
        layer: raw.layer,
        proof_exports: raw.proof_exports,
    })
}

/// Discover and parse every layer manifest at `<root>/crates/*/layer.toml`.
///
/// Discovery is deliberately one level deep: it never recurses into nested
/// directories (such as `crates/xtask/tests/fixtures/...`), so a real run from
/// the repo root sees only the real layer crates. Returns parsed manifests and
/// any parse errors separately; both are sorted by path for determinism.
pub fn load_manifests(root: &Path) -> (Vec<LayerManifest>, Vec<ManifestError>) {
    let crates_dir = root.join("crates");
    let mut manifests = Vec::new();
    let mut errors = Vec::new();

    let entries = match std::fs::read_dir(&crates_dir) {
        Ok(entries) => entries,
        // No `crates/` dir means no layers to check; not an error here.
        Err(_) => return (manifests, errors),
    };

    let mut crate_dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    crate_dirs.sort();

    for crate_dir in crate_dirs {
        let manifest_path = crate_dir.join("layer.toml");
        if !manifest_path.is_file() {
            continue; // Not a layer (e.g. the xtask tooling crate).
        }
        match std::fs::read_to_string(&manifest_path) {
            Ok(text) => match parse_manifest(&crate_dir, &text) {
                Ok(manifest) => manifests.push(manifest),
                Err(err) => errors.push(err),
            },
            Err(e) => errors.push(ManifestError {
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
    fn parses_a_full_manifest() {
        let text = r#"
            [layer]
            name = "runtime"
            index = 1
            previous = "kernel"
            crate_name = "axiom-runtime"
            allowed_dependencies = ["kernel"]
            forbidden_dependencies = []
            meaningful_dependency = "Runtime steps the deterministic kernel clock."
            introduced_capabilities = ["Runtime"]
            consumed_capabilities = ["KernelApi"]

            [[proof_exports]]
            export = "Runtime"
            must_reference = ["KernelApi"]
        "#;
        let m = parse_manifest(Path::new("crates/axiom-runtime"), text).unwrap();
        assert_eq!(m.layer.name, "runtime");
        assert_eq!(m.layer.index, 1);
        assert_eq!(m.layer.previous.as_deref(), Some("kernel"));
        assert_eq!(m.import_prefix(), "axiom_runtime");
        assert_eq!(m.proof_exports.len(), 1);
        assert_eq!(m.proof_exports[0].export, "Runtime");
        assert_eq!(m.proof_exports[0].must_reference, vec!["KernelApi"]);
    }

    #[test]
    fn import_prefix_defaults_to_axiom_name() {
        let text = r#"
            [layer]
            name = "kernel"
            index = 0
            meaningful_dependency = "Base layer."
        "#;
        let m = parse_manifest(Path::new("crates/axiom-kernel"), text).unwrap();
        assert_eq!(m.import_prefix(), "axiom_kernel");
    }

    #[test]
    fn unknown_field_is_rejected() {
        let text = r#"
            [layer]
            name = "kernel"
            index = 0
            meaningful_dependency = "Base."
            surprise = true
        "#;
        assert!(parse_manifest(Path::new("crates/axiom-kernel"), text).is_err());
    }
}
