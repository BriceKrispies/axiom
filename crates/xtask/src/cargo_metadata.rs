//! Minimal `cargo metadata` integration.
//!
//! Shells out to `cargo metadata --format-version=1 --no-deps` and parses
//! the subset of the output the architecture checker needs: workspace
//! members, their manifest paths, and each member's directly-declared
//! workspace-internal dependencies.
//!
//! The cargo CLI is the source of truth. If `cargo metadata` is
//! unavailable or fails (e.g. on a synthetic fixture that has no
//! `Cargo.toml`), [`load`] returns a [`MetadataError`] and the caller can
//! decide to proceed with the manifest-only checks instead.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// The workspace dependency graph as the architecture checker sees it.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceGraph {
    pub root: PathBuf,
    pub packages: Vec<WorkspacePackage>,
}

/// One workspace member as exposed by `cargo metadata`.
#[derive(Debug, Clone)]
pub struct WorkspacePackage {
    /// Cargo package name (e.g. `"axiom-kernel"`).
    pub name: String,
    /// The directory containing this package's `Cargo.toml`.
    pub dir: PathBuf,
    /// Workspace-internal direct dependencies (sorted, deduplicated).
    /// Excludes external registry crates.
    pub workspace_deps: Vec<String>,
}

/// An error talking to `cargo metadata` or parsing its output.
#[derive(Debug)]
pub struct MetadataError {
    pub message: String,
}

impl std::fmt::Display for MetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Load the workspace graph rooted at `root`.
///
/// Tries `cargo metadata` first (the authoritative source). If that
/// fails or `cargo` is unavailable, falls back to a pure-TOML parse via
/// [`load_via_toml`] so synthetic fixtures and CI environments without a
/// working cargo install still validate.
pub fn load(root: &Path) -> Result<WorkspaceGraph, MetadataError> {
    match load_via_cargo(root) {
        Ok(g) => Ok(g),
        Err(_) => load_via_toml(root),
    }
}

/// Manifest-only workspace graph loader: parses the workspace's
/// `Cargo.toml` files directly without invoking `cargo`. Resolves path
/// deps to workspace package names by reading each member's
/// `[package].name`.
pub fn load_via_toml(root: &Path) -> Result<WorkspaceGraph, MetadataError> {
    use serde::Deserialize;
    use std::collections::BTreeMap;

    #[derive(Deserialize)]
    struct RootCargo {
        workspace: WorkspaceTable,
    }
    #[derive(Deserialize)]
    struct WorkspaceTable {
        #[serde(default)]
        members: Vec<String>,
    }
    #[derive(Deserialize)]
    struct MemberCargo {
        #[serde(default)]
        package: Option<PackageTable>,
        #[serde(default)]
        dependencies: BTreeMap<String, DepValue>,
    }
    #[derive(Deserialize)]
    struct PackageTable {
        name: String,
    }
    #[derive(Deserialize)]
    #[serde(untagged)]
    #[allow(dead_code)]
    enum DepValue {
        Version(String),
        Detailed(DepTable),
    }
    #[derive(Deserialize, Default)]
    struct DepTable {
        #[serde(default)]
        package: Option<String>,
    }

    let root_cargo_path = root.join("Cargo.toml");
    let root_text = std::fs::read_to_string(&root_cargo_path).map_err(|e| MetadataError {
        message: format!(
            "could not read workspace Cargo.toml at {}: {e}",
            root_cargo_path.display()
        ),
    })?;
    let root_cargo: RootCargo = toml::from_str(&root_text).map_err(|e| MetadataError {
        message: format!(
            "could not parse workspace Cargo.toml at {}: {}",
            root_cargo_path.display(),
            e.message()
        ),
    })?;

    let mut dir_by_name: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut members: Vec<(PathBuf, String, MemberCargo)> = Vec::new();
    for member_dir_rel in &root_cargo.workspace.members {
        let member_dir = root.join(member_dir_rel);
        let manifest = member_dir.join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest).map_err(|e| MetadataError {
            message: format!(
                "could not read member Cargo.toml at {}: {e}",
                manifest.display()
            ),
        })?;
        let parsed: MemberCargo = toml::from_str(&text).map_err(|e| MetadataError {
            message: format!(
                "could not parse member Cargo.toml at {}: {}",
                manifest.display(),
                e.message()
            ),
        })?;
        let name = parsed
            .package
            .as_ref()
            .map(|p| p.name.clone())
            .ok_or_else(|| MetadataError {
                message: format!("member at {} has no `[package].name`", manifest.display()),
            })?;
        dir_by_name.insert(name.clone(), member_dir.clone());
        members.push((member_dir, name, parsed));
    }

    let mut packages = Vec::new();
    for (dir, name, parsed) in &members {
        let mut workspace_deps: BTreeSet<String> = BTreeSet::new();
        for (dep_key, value) in &parsed.dependencies {
            let rename = match value {
                DepValue::Version(_) => None,
                DepValue::Detailed(t) => t.package.clone(),
            };
            let resolved = rename.unwrap_or_else(|| dep_key.clone());
            if dir_by_name.contains_key(&resolved) {
                workspace_deps.insert(resolved);
            }
        }
        packages.push(WorkspacePackage {
            name: name.clone(),
            dir: dir.clone(),
            workspace_deps: workspace_deps.into_iter().collect(),
        });
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(WorkspaceGraph {
        root: root.to_path_buf(),
        packages,
    })
}

fn load_via_cargo(root: &Path) -> Result<WorkspaceGraph, MetadataError> {
    let manifest = root.join("Cargo.toml");
    if !manifest.is_file() {
        return Err(MetadataError {
            message: format!(
                "no workspace Cargo.toml at {} — cannot run `cargo metadata`",
                manifest.display()
            ),
        });
    }

    let cargo_bin = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(&cargo_bin)
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--no-deps")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .map_err(|e| MetadataError {
            message: format!("could not invoke `{cargo_bin} metadata`: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MetadataError {
            message: format!(
                "`cargo metadata` exited with status {}: {}",
                output.status, stderr
            ),
        });
    }

    parse_metadata(&output.stdout).map(|mut g| {
        g.root = root.to_path_buf();
        g
    })
}

#[derive(Debug, Deserialize)]
struct RawMetadata {
    packages: Vec<RawPackage>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawPackage {
    name: String,
    id: String,
    manifest_path: String,
    #[serde(default)]
    dependencies: Vec<RawDependency>,
}

#[derive(Debug, Deserialize)]
struct RawDependency {
    name: String,
    /// `null` for path / workspace deps; a registry URL for external deps.
    #[serde(default)]
    source: Option<String>,
    /// `null` for normal deps; `"dev"` for dev-dependencies; `"build"` for
    /// build-dependencies. The architecture checker treats normal deps as
    /// load-bearing and ignores dev-deps (they live behind `#[cfg(test)]`).
    #[serde(default)]
    kind: Option<String>,
}

fn parse_metadata(bytes: &[u8]) -> Result<WorkspaceGraph, MetadataError> {
    let raw: RawMetadata = serde_json::from_slice(bytes).map_err(|e| MetadataError {
        message: format!("could not parse `cargo metadata` output as JSON: {e}"),
    })?;

    let workspace_ids: BTreeSet<&str> = raw.workspace_members.iter().map(String::as_str).collect();
    let workspace_names: BTreeSet<&str> = raw
        .packages
        .iter()
        .filter(|p| workspace_ids.contains(p.id.as_str()))
        .map(|p| p.name.as_str())
        .collect();

    let mut packages = Vec::new();
    for raw_pkg in &raw.packages {
        if !workspace_ids.contains(raw_pkg.id.as_str()) {
            continue;
        }
        let manifest_path = PathBuf::from(&raw_pkg.manifest_path);
        let dir = manifest_path
            .parent()
            .ok_or_else(|| MetadataError {
                message: format!(
                    "package `{}` manifest_path has no parent dir: {}",
                    raw_pkg.name,
                    manifest_path.display()
                ),
            })?
            .to_path_buf();

        let mut workspace_deps: BTreeSet<String> = BTreeSet::new();
        for dep in &raw_pkg.dependencies {
            // Ignore dev-deps and build-deps; only normal deps load-bear.
            if dep.kind.is_some() {
                continue;
            }
            // Path/workspace deps have `source = null`. Cross-checked with
            // the workspace name set so a renamed dep doesn't slip through.
            if dep.source.is_some() {
                continue;
            }
            if workspace_names.contains(dep.name.as_str()) {
                workspace_deps.insert(dep.name.clone());
            }
        }

        packages.push(WorkspacePackage {
            name: raw_pkg.name.clone(),
            dir,
            workspace_deps: workspace_deps.into_iter().collect(),
        });
    }

    packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(WorkspaceGraph {
        root: PathBuf::new(),
        packages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_metadata_json() {
        let json = br#"{
            "packages": [
                {
                    "name": "alpha",
                    "id": "alpha 0.1.0 (path+file:///alpha)",
                    "manifest_path": "/alpha/Cargo.toml",
                    "dependencies": []
                },
                {
                    "name": "beta",
                    "id": "beta 0.1.0 (path+file:///beta)",
                    "manifest_path": "/beta/Cargo.toml",
                    "dependencies": [
                        {"name": "alpha", "source": null, "kind": null}
                    ]
                }
            ],
            "workspace_members": [
                "alpha 0.1.0 (path+file:///alpha)",
                "beta 0.1.0 (path+file:///beta)"
            ]
        }"#;
        let g = parse_metadata(json).unwrap();
        assert_eq!(g.packages.len(), 2);
        let beta = g.packages.iter().find(|p| p.name == "beta").unwrap();
        assert_eq!(beta.workspace_deps, vec!["alpha".to_string()]);
    }

    #[test]
    fn dev_deps_are_ignored_for_load_bearing_classification() {
        let json = br#"{
            "packages": [
                {
                    "name": "alpha",
                    "id": "alpha 0.1.0 (path+file:///alpha)",
                    "manifest_path": "/alpha/Cargo.toml",
                    "dependencies": []
                },
                {
                    "name": "beta",
                    "id": "beta 0.1.0 (path+file:///beta)",
                    "manifest_path": "/beta/Cargo.toml",
                    "dependencies": [
                        {"name": "alpha", "source": null, "kind": "dev"}
                    ]
                }
            ],
            "workspace_members": [
                "alpha 0.1.0 (path+file:///alpha)",
                "beta 0.1.0 (path+file:///beta)"
            ]
        }"#;
        let g = parse_metadata(json).unwrap();
        let beta = g.packages.iter().find(|p| p.name == "beta").unwrap();
        assert!(beta.workspace_deps.is_empty());
    }

    #[test]
    fn external_registry_deps_are_filtered_out() {
        let json = br#"{
            "packages": [
                {
                    "name": "alpha",
                    "id": "alpha 0.1.0 (path+file:///alpha)",
                    "manifest_path": "/alpha/Cargo.toml",
                    "dependencies": [
                        {"name": "serde", "source": "registry+https://github.com/rust-lang/crates.io-index", "kind": null}
                    ]
                }
            ],
            "workspace_members": [
                "alpha 0.1.0 (path+file:///alpha)"
            ]
        }"#;
        let g = parse_metadata(json).unwrap();
        assert!(g.packages[0].workspace_deps.is_empty());
    }

    #[test]
    fn missing_workspace_cargo_toml_is_an_error() {
        let tmp = std::env::temp_dir().join("axiom_xtask_no_workspace_cargo");
        let _ = std::fs::create_dir_all(&tmp);
        let err = load(&tmp).unwrap_err();
        // The cargo path errors with "no workspace Cargo.toml" and the toml
        // fallback errors with "could not read workspace Cargo.toml". Either
        // is acceptable as a structured failure shape.
        assert!(
            err.message.contains("Cargo.toml"),
            "expected an error mentioning Cargo.toml, got: {}",
            err.message
        );
    }

    #[test]
    fn load_via_toml_parses_a_simple_workspace() {
        let tmp = std::env::temp_dir().join("axiom_xtask_toml_ws");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("a/src")).unwrap();
        std::fs::create_dir_all(tmp.join("b/src")).unwrap();
        std::fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"a\", \"b\"]\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("a/Cargo.toml"),
            "[package]\nname=\"a\"\nversion=\"0.0.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("b/Cargo.toml"),
            "[package]\nname=\"b\"\nversion=\"0.0.0\"\nedition=\"2021\"\n\n[dependencies]\na = { path = \"../a\" }\n",
        )
        .unwrap();
        std::fs::write(tmp.join("a/src/lib.rs"), "").unwrap();
        std::fs::write(tmp.join("b/src/lib.rs"), "").unwrap();
        let g = load_via_toml(&tmp).unwrap();
        assert_eq!(g.packages.len(), 2);
        let b = g.packages.iter().find(|p| p.name == "b").unwrap();
        assert_eq!(b.workspace_deps, vec!["a".to_string()]);
    }
}
