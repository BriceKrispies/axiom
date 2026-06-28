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
///
/// Layers form a directed *acyclic* graph, not a strict line: each layer lists
/// the layers it directly depends on in `depends_on` and must genuinely
/// use each one (the latter enforced by the `engine_genuine_dependency` dylint).
/// A layer with an empty `depends_on` is a root (e.g. the kernel).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerSection {
    pub name: String,
    #[serde(default)]
    pub crate_name: Option<String>,
    /// The layers this layer directly depends on. These are exactly the
    /// layers it may import, and each must be genuinely used.
    #[serde(default)]
    pub depends_on: Vec<String>,
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
    toml::from_str::<RawManifest>(text)
        .map_err(|e| ManifestError {
            path: dir.join("layer.toml"),
            message: e.message().to_string(),
        })
        .map(|raw| LayerManifest {
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

    // No `crates/` dir means no layers to check; `read_dir`'s `Result` flattens
    // to zero entries on `Err`.
    let mut crate_dirs: Vec<PathBuf> = std::fs::read_dir(&crates_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    crate_dirs.sort();

    // Each crate dir with a `layer.toml` yields one parse result; split the
    // Ok/Err results into the two output vecs without branching. Dirs lacking a
    // `layer.toml` (e.g. the xtask tooling crate) are skipped.
    crate_dirs
        .into_iter()
        .map(|crate_dir| crate_dir.join("layer.toml"))
        .filter(|manifest_path| manifest_path.is_file())
        .map(|manifest_path| {
            let crate_dir = manifest_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            std::fs::read_to_string(&manifest_path)
                .map_err(|e| ManifestError {
                    path: manifest_path,
                    message: format!("could not read file: {e}"),
                })
                .and_then(|text| parse_manifest(&crate_dir, &text))
        })
        .fold(
            (Vec::new(), Vec::new()),
            |(mut manifests, mut errors), result| {
                result
                    .map(|manifest| manifests.push(manifest))
                    .unwrap_or_else(|err| errors.push(err));
                (manifests, errors)
            },
        )
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
            crate_name = "axiom-runtime"
            depends_on = ["kernel"]
            meaningful_dependency = "Runtime steps the deterministic kernel clock."
            introduced_capabilities = ["Runtime"]
            consumed_capabilities = ["KernelApi"]

            [[proof_exports]]
            export = "Runtime"
            must_reference = ["KernelApi"]
        "#;
        let m = parse_manifest(Path::new("crates/axiom-runtime"), text).unwrap();
        assert_eq!(m.layer.name, "runtime");
        assert_eq!(m.layer.depends_on, vec!["kernel"]);
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
            meaningful_dependency = "Base layer."
        "#;
        let m = parse_manifest(Path::new("crates/axiom-kernel"), text).unwrap();
        assert_eq!(m.import_prefix(), "axiom_kernel");
        assert!(m.layer.depends_on.is_empty());
    }

    #[test]
    fn unknown_field_is_rejected() {
        let text = r#"
            [layer]
            name = "kernel"
            meaningful_dependency = "Base."
            surprise = true
        "#;
        assert!(parse_manifest(Path::new("crates/axiom-kernel"), text).is_err());
    }
}
