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
    /// Game names this app (a host) may load — the cartridges it hosts. Empty
    /// for a non-host app. An app may Cargo-depend on a game only if that game's
    /// logical name is listed here.
    #[serde(default)]
    pub allowed_games: Vec<String>,
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
    toml::from_str::<RawManifest>(text)
        .map_err(|e| AppManifestError {
            path: dir.join("app.toml"),
            message: e.message().to_string(),
        })
        .map(|raw| AppManifest {
            dir: dir.to_path_buf(),
            app: raw.app,
        })
}

/// Discover and parse every app manifest at `<root>/apps/*/app.toml`.
pub fn load_app_manifests(root: &Path) -> (Vec<AppManifest>, Vec<AppManifestError>) {
    let apps_dir = root.join("apps");

    // A missing `apps/` dir yields nothing; `read_dir`'s `Result` flattens to
    // zero entries on `Err`.
    let mut crate_dirs: Vec<PathBuf> = std::fs::read_dir(&apps_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    crate_dirs.sort();

    // Each crate dir with an `app.toml` yields one parse result, split into
    // the two output vecs below.
    crate_dirs
        .into_iter()
        .map(|crate_dir| crate_dir.join("app.toml"))
        .filter(|manifest_path| manifest_path.is_file())
        .map(|manifest_path| {
            let crate_dir = manifest_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            std::fs::read_to_string(&manifest_path)
                .map_err(|e| AppManifestError {
                    path: manifest_path,
                    message: format!("could not read file: {e}"),
                })
                .and_then(|text| parse_app_manifest(&crate_dir, &text))
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
    fn parses_a_full_app_manifest() {
        let text = r#"
            [app]
            name = "rotating-cube-demo"
            crate_name = "axiom-demo-rotating-cube"
            allowed_layers = ["kernel", "runtime", "math", "host", "frame"]
            allowed_modules = ["scene", "render"]
            allowed_games = ["retro_fps"]
        "#;
        let a = parse_app_manifest(Path::new("apps/rotating-cube"), text).unwrap();
        assert_eq!(a.app.name, "rotating-cube-demo");
        assert_eq!(a.app.crate_name, "axiom-demo-rotating-cube");
        assert_eq!(a.app.allowed_layers.len(), 5);
        assert_eq!(a.app.allowed_modules, vec!["scene", "render"]);
        assert_eq!(a.app.allowed_games, vec!["retro_fps"]);
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
