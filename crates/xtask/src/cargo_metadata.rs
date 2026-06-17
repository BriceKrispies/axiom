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
    // Cargo is authoritative; fall back to a pure-TOML parse on any failure.
    load_via_cargo(root).or_else(|_| load_via_toml(root))
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
    // Read + parse the workspace manifest, then read + parse each member; both
    // are fallible, so the whole pipeline is an error-propagating chain.
    let root_text = std::fs::read_to_string(&root_cargo_path).map_err(|e| MetadataError {
        message: format!(
            "could not read workspace Cargo.toml at {}: {e}",
            root_cargo_path.display()
        ),
    });
    let root_cargo = root_text.and_then(|root_text| {
        toml::from_str::<RootCargo>(&root_text).map_err(|e| MetadataError {
            message: format!(
                "could not parse workspace Cargo.toml at {}: {}",
                root_cargo_path.display(),
                e.message()
            ),
        })
    });

    let members = root_cargo.and_then(|root_cargo| {
        root_cargo
            .workspace
            .members
            .iter()
            .map(|member_dir_rel| {
                let member_dir = root.join(member_dir_rel);
                let manifest = member_dir.join("Cargo.toml");
                std::fs::read_to_string(&manifest)
                    .map_err(|e| MetadataError {
                        message: format!(
                            "could not read member Cargo.toml at {}: {e}",
                            manifest.display()
                        ),
                    })
                    .and_then(|text| {
                        toml::from_str::<MemberCargo>(&text).map_err(|e| MetadataError {
                            message: format!(
                                "could not parse member Cargo.toml at {}: {}",
                                manifest.display(),
                                e.message()
                            ),
                        })
                    })
                    .and_then(|parsed| {
                        parsed
                            .package
                            .as_ref()
                            .map(|p| p.name.clone())
                            .ok_or_else(|| MetadataError {
                                message: format!(
                                    "member at {} has no `[package].name`",
                                    manifest.display()
                                ),
                            })
                            .map(|name| (member_dir, name, parsed))
                    })
            })
            .collect::<Result<Vec<(PathBuf, String, MemberCargo)>, MetadataError>>()
    });

    members.map(|members| {
        let dir_by_name: BTreeMap<String, PathBuf> = members
            .iter()
            .map(|(dir, name, _)| (name.clone(), dir.clone()))
            .collect();

        let mut packages: Vec<WorkspacePackage> = members
            .iter()
            .map(|(dir, name, parsed)| {
                let workspace_deps: Vec<String> = parsed
                    .dependencies
                    .iter()
                    .filter_map(|(dep_key, value)| {
                        // A renamed dep (`package = "…"`) resolves to that name;
                        // otherwise the key is the crate name.
                        let resolved = match value {
                            DepValue::Version(_) => None,
                            DepValue::Detailed(t) => t.package.clone(),
                        }
                        .unwrap_or_else(|| dep_key.clone());
                        dir_by_name.contains_key(&resolved).then_some(resolved)
                    })
                    .collect::<BTreeSet<String>>()
                    .into_iter()
                    .collect();
                WorkspacePackage {
                    name: name.clone(),
                    dir: dir.clone(),
                    workspace_deps,
                }
            })
            .collect();
        packages.sort_by(|a, b| a.name.cmp(&b.name));

        WorkspaceGraph {
            root: root.to_path_buf(),
            packages,
        }
    })
}

fn load_via_cargo(root: &Path) -> Result<WorkspaceGraph, MetadataError> {
    let manifest = root.join("Cargo.toml");
    let cargo_bin = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    // Require the manifest, run `cargo metadata`, require success, then parse —
    // each step error-propagates into the next.
    manifest
        .is_file()
        .then_some(())
        .ok_or_else(|| MetadataError {
            message: format!(
                "no workspace Cargo.toml at {} — cannot run `cargo metadata`",
                manifest.display()
            ),
        })
        .and_then(|()| {
            Command::new(&cargo_bin)
                .arg("metadata")
                .arg("--format-version=1")
                .arg("--no-deps")
                .arg("--offline")
                .arg("--manifest-path")
                .arg(&manifest)
                .output()
                .map_err(|e| MetadataError {
                    message: format!("could not invoke `{cargo_bin} metadata`: {e}"),
                })
        })
        .and_then(|output| {
            let status = output.status;
            let stdout = output.stdout;
            let stderr = output.stderr;
            status.success().then_some(stdout).ok_or_else(|| MetadataError {
                message: format!(
                    "`cargo metadata` exited with status {status}: {}",
                    String::from_utf8_lossy(&stderr)
                ),
            })
        })
        .and_then(|stdout| {
            parse_metadata(&stdout).map(|mut g| {
                g.root = root.to_path_buf();
                g
            })
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
    let raw = serde_json::from_slice::<RawMetadata>(bytes).map_err(|e| MetadataError {
        message: format!("could not parse `cargo metadata` output as JSON: {e}"),
    });

    raw.and_then(|raw| {
        let workspace_ids: BTreeSet<&str> =
            raw.workspace_members.iter().map(String::as_str).collect();
        let workspace_names: BTreeSet<&str> = raw
            .packages
            .iter()
            .filter(|p| workspace_ids.contains(p.id.as_str()))
            .map(|p| p.name.as_str())
            .collect();

        // Keep only workspace members; each resolves its parent dir (fallible).
        raw.packages
            .iter()
            .filter(|raw_pkg| workspace_ids.contains(raw_pkg.id.as_str()))
            .map(|raw_pkg| {
                let manifest_path = PathBuf::from(&raw_pkg.manifest_path);
                manifest_path
                    .parent()
                    .map(Path::to_path_buf)
                    .ok_or_else(|| MetadataError {
                        message: format!(
                            "package `{}` manifest_path has no parent dir: {}",
                            raw_pkg.name,
                            manifest_path.display()
                        ),
                    })
                    .map(|dir| {
                        // Normal deps only (no dev/build, source = null), and
                        // cross-checked against the workspace name set.
                        let workspace_deps: Vec<String> = raw_pkg
                            .dependencies
                            .iter()
                            .filter(|dep| dep.kind.is_none() & dep.source.is_none())
                            .filter(|dep| workspace_names.contains(dep.name.as_str()))
                            .map(|dep| dep.name.clone())
                            .collect::<BTreeSet<String>>()
                            .into_iter()
                            .collect();
                        WorkspacePackage {
                            name: raw_pkg.name.clone(),
                            dir,
                            workspace_deps,
                        }
                    })
            })
            .collect::<Result<Vec<WorkspacePackage>, MetadataError>>()
            .map(|mut packages| {
                packages.sort_by(|a, b| a.name.cmp(&b.name));
                WorkspaceGraph {
                    root: PathBuf::new(),
                    packages,
                }
            })
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
