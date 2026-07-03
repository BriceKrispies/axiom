//! The `game.toml` manifest schema and loader.
//!
//! One manifest lives in each game crate at `games/<game>/game.toml`. A **game**
//! is the cartridge tier: a title built *on* the engine (it composes layers and
//! modules like an app does), but — unlike an app — it is **not a leaf**. Hosts
//! (the gallery showcase, the workspace dev console, the game-runtime) may depend
//! on a game and load it; the engine spine (layers/modules) never may. A game is
//! content, not a reusable engine capability, so it sits in its own class rather
//! than being forced into `modules/` (a capability) or `apps/` (a leaf).

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// A parsed `game.toml`, paired with the directory it was found in.
#[derive(Debug, Clone)]
pub struct GameManifest {
    pub dir: PathBuf,
    pub game: GameSection,
}

impl GameManifest {
    pub fn import_prefix(&self) -> String {
        self.game.crate_name.replace('-', "_")
    }

    pub fn src_dir(&self) -> PathBuf {
        self.dir.join("src")
    }
}

/// The `[game]` table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameSection {
    /// Short logical game name (e.g. `"retro_fps"`).
    pub name: String,
    /// The actual cargo package name (e.g. `"axiom-game-retro-fps"`).
    pub crate_name: String,
    /// The authoring lane: `"rust"` (a crate exposing `fn app() -> App`) or
    /// `"bundle"` (a `@axiom/game` TS bundle hosted by `axiom-game-runtime`).
    /// Defaults to `"rust"`.
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Layer names this game may depend on.
    #[serde(default)]
    pub allowed_layers: Vec<String>,
    /// Module names this game may depend on.
    #[serde(default)]
    pub allowed_modules: Vec<String>,
}

fn default_kind() -> String {
    "rust".to_string()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    game: GameSection,
}

#[derive(Debug)]
pub struct GameManifestError {
    pub path: PathBuf,
    pub message: String,
}

pub fn parse_game_manifest(dir: &Path, text: &str) -> Result<GameManifest, GameManifestError> {
    toml::from_str::<RawManifest>(text)
        .map_err(|e| GameManifestError {
            path: dir.join("game.toml"),
            message: e.message().to_string(),
        })
        .map(|raw| GameManifest {
            dir: dir.to_path_buf(),
            game: raw.game,
        })
}

/// Discover and parse every game manifest at `<root>/games/*/game.toml`.
pub fn load_game_manifests(root: &Path) -> (Vec<GameManifest>, Vec<GameManifestError>) {
    let games_dir = root.join("games");

    // A missing `games/` dir yields nothing; `read_dir`'s `Result` flattens to
    // zero entries on `Err`.
    let mut crate_dirs: Vec<PathBuf> = std::fs::read_dir(&games_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    crate_dirs.sort();

    // Each crate dir with a `game.toml` yields one parse result, split into the
    // two output vecs below.
    crate_dirs
        .into_iter()
        .map(|crate_dir| crate_dir.join("game.toml"))
        .filter(|manifest_path| manifest_path.is_file())
        .map(|manifest_path| {
            let crate_dir = manifest_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            std::fs::read_to_string(&manifest_path)
                .map_err(|e| GameManifestError {
                    path: manifest_path,
                    message: format!("could not read file: {e}"),
                })
                .and_then(|text| parse_game_manifest(&crate_dir, &text))
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
    fn parses_a_full_game_manifest() {
        let text = r#"
            [game]
            name = "retro_fps"
            crate_name = "axiom-game-retro-fps"
            kind = "rust"
            allowed_layers = ["kernel", "runtime", "math", "host", "frame"]
            allowed_modules = ["engine", "windowing"]
        "#;
        let g = parse_game_manifest(Path::new("games/retro-fps"), text).unwrap();
        assert_eq!(g.game.name, "retro_fps");
        assert_eq!(g.game.crate_name, "axiom-game-retro-fps");
        assert_eq!(g.game.kind, "rust");
        assert_eq!(g.game.allowed_layers.len(), 5);
        assert_eq!(g.game.allowed_modules, vec!["engine", "windowing"]);
        assert_eq!(g.import_prefix(), "axiom_game_retro_fps");
        assert_eq!(g.src_dir(), Path::new("games/retro-fps/src"));
    }

    #[test]
    fn kind_defaults_to_rust() {
        let text = r#"
            [game]
            name = "retro_fps"
            crate_name = "axiom-game-retro-fps"
        "#;
        let g = parse_game_manifest(Path::new("games/retro-fps"), text).unwrap();
        assert_eq!(g.game.kind, "rust");
        assert!(g.game.allowed_layers.is_empty());
        assert!(g.game.allowed_modules.is_empty());
    }

    #[test]
    fn unknown_field_is_rejected() {
        let text = r#"
            [game]
            name = "retro_fps"
            crate_name = "axiom-game-retro-fps"
            mystery = true
        "#;
        assert!(parse_game_manifest(Path::new("games/retro-fps"), text).is_err());
    }
}
