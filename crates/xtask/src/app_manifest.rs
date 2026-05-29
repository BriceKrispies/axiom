//! The `app.toml` manifest schema and loader.
//!
//! One manifest lives in each app crate at `apps/<app>/app.toml`. Apps are
//! the **only** composition roots in the workspace: they may depend on
//! layers and modules, but nothing may depend on them.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// A parsed `app.toml`, paired with the directory it was found in.
#[derive(Debug, Clone)]
pub struct AppManifest {
    pub dir: PathBuf,
    pub app: AppSection,
}

impl AppManifest {
    pub fn import_prefix(&self) -> String {
        self.app.crate_name.replace('-', "_")
    }

    pub fn src_dir(&self) -> PathBuf {
        self.dir.join("src")
    }
}

/// The `[app]` table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSection {
    /// Short logical app name (e.g. `"rotating-cube-demo"`).
    pub name: String,
    /// The actual cargo package name (e.g. `"axiom-demo-rotating-cube"`).
    pub crate_name: String,
    /// Layer names this app may depend on.
    #[serde(default)]
    pub allowed_layers: Vec<String>,
    /// Module names this app may depend on.
    #[serde(default)]
    pub allowed_modules: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    app: AppSection,
}

#[derive(Debug)]
pub struct AppManifestError {
    pub path: PathBuf,
    pub message: String,
}

pub fn parse_app_manifest(dir: &Path, text: &str) -> Result<AppManifest, AppManifestError> {
    let raw: RawManifest = toml::from_str(text).map_err(|e| AppManifestError {
        path: dir.join("app.toml"),
        message: e.message().to_string(),
    })?;
    Ok(AppManifest {
        dir: dir.to_path_buf(),
        app: raw.app,
    })
}

/// Discover and parse every app manifest at `<root>/apps/*/app.toml`.
pub fn load_app_manifests(root: &Path) -> (Vec<AppManifest>, Vec<AppManifestError>) {
    let apps_dir = root.join("apps");
    let mut manifests = Vec::new();
    let mut errors = Vec::new();

    let entries = match std::fs::read_dir(&apps_dir) {
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
        let manifest_path = crate_dir.join("app.toml");
        if !manifest_path.is_file() {
            continue;
        }
        match std::fs::read_to_string(&manifest_path) {
            Ok(text) => match parse_app_manifest(&crate_dir, &text) {
                Ok(manifest) => manifests.push(manifest),
                Err(err) => errors.push(err),
            },
            Err(e) => errors.push(AppManifestError {
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
    fn parses_a_full_app_manifest() {
        let text = r#"
            [app]
            name = "rotating-cube-demo"
            crate_name = "axiom-demo-rotating-cube"
            allowed_layers = ["kernel", "runtime", "math", "host", "frame"]
            allowed_modules = ["scene", "render"]
        "#;
        let a = parse_app_manifest(Path::new("apps/rotating-cube"), text).unwrap();
        assert_eq!(a.app.name, "rotating-cube-demo");
        assert_eq!(a.app.crate_name, "axiom-demo-rotating-cube");
        assert_eq!(a.app.allowed_layers.len(), 5);
        assert_eq!(a.app.allowed_modules, vec!["scene", "render"]);
        assert_eq!(a.import_prefix(), "axiom_demo_rotating_cube");
    }

    #[test]
    fn unknown_field_is_rejected() {
        let text = r#"
            [app]
            name = "demo"
            crate_name = "axiom-demo"
            mystery = true
        "#;
        assert!(parse_app_manifest(Path::new("apps/demo"), text).is_err());
    }
}
