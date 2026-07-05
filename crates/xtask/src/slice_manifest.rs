//! The `slice.toml` manifest schema and loader.
//!
//! One manifest lives in each *renderable slice* — an app or game that renders a
//! visible result and proves it deterministic — at `apps/<name>/slice.toml` or
//! `games/<name>/slice.toml`. Where `app.toml`/`game.toml` describe a package's
//! *structural* place in the dependency graph, `slice.toml` describes its
//! *semantic* vertical-slice contract: which determinism test proves it, which
//! committed golden `.bin` artifacts it pins (each to a recorded SHA-256, so a
//! deleted or regenerated golden is caught by the checker rather than silently
//! re-blessed), an optional reference image (also hash-pinned), the public
//! `harness_entry` symbol that is its renderable core, and the optional
//! `harness` name under which `axiom-shot` renders it.
//!
//! The `xtask check-slices` subcommand (see [`crate::slice_check`]) enforces
//! this contract; nothing here interprets it, only parses it.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// A parsed `slice.toml`, paired with the directory it was found in (the owning
/// app/game crate directory — golden/reference/test paths resolve against it).
#[derive(Debug, Clone)]
pub struct SliceManifest {
    pub dir: PathBuf,
    pub slice: SliceSection,
    pub goldens: Vec<GoldenEntry>,
    pub reference: Option<ReferenceEntry>,
}

impl SliceManifest {
    pub fn src_dir(&self) -> PathBuf {
        self.dir.join("src")
    }

    /// The determinism test file this slice declares
    /// (`<dir>/tests/<determinism_test>.rs`).
    pub fn determinism_test_path(&self) -> PathBuf {
        self.dir
            .join("tests")
            .join(format!("{}.rs", self.slice.determinism_test))
    }
}

/// The `[slice]` table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceSection {
    /// Short logical slice name (matches the owning app/game name).
    pub name: String,
    /// The cargo package that owns this slice (its determinism test + goldens).
    pub crate_name: String,
    /// A public symbol in `crate_name`'s source that is the slice's renderable
    /// core — the proof the harness entry exists (e.g. `build_retro_fps_app`).
    pub harness_entry: String,
    /// The determinism test target: `<dir>/tests/<determinism_test>.rs` must
    /// exist. That test proves a fixed scenario replays byte-equal AND a
    /// perturbed run differs.
    pub determinism_test: String,
    /// The name under which `axiom-shot` registers this slice, when it has a
    /// live pixel harness. When set, `check-slices` also asserts it is present
    /// in the axiom-shot registry. Omitted for a headless slice (one whose
    /// determinism is proven by committed artifacts but which has no live
    /// renderable core in its own crate).
    #[serde(default)]
    pub harness: Option<String>,
}

/// One committed golden `.bin` artifact, pinned to a SHA-256.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenEntry {
    /// Path to the golden file, relative to the slice's crate directory.
    pub path: String,
    /// The lowercase hex SHA-256 the committed file must hash to.
    pub sha256: String,
}

/// A reference image (real harness pixels), pinned to a SHA-256.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceEntry {
    /// Path to the reference image, relative to the slice's crate directory.
    pub path: String,
    /// The lowercase hex SHA-256 the committed image must hash to.
    pub sha256: String,
    /// Which harness produced the reference (documentation; e.g. `axiom-shot`).
    #[serde(default)]
    pub harness: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    slice: SliceSection,
    #[serde(default)]
    golden: Vec<GoldenEntry>,
    #[serde(default)]
    reference: Option<ReferenceEntry>,
}

#[derive(Debug)]
pub struct SliceManifestError {
    pub path: PathBuf,
    pub message: String,
}

pub fn parse_slice_manifest(dir: &Path, text: &str) -> Result<SliceManifest, SliceManifestError> {
    toml::from_str::<RawManifest>(text)
        .map_err(|e| SliceManifestError {
            path: dir.join("slice.toml"),
            message: e.message().to_string(),
        })
        .map(|raw| SliceManifest {
            dir: dir.to_path_buf(),
            slice: raw.slice,
            goldens: raw.golden,
            reference: raw.reference,
        })
}

/// Discover and parse every slice manifest at `<root>/apps/*/slice.toml` and
/// `<root>/games/*/slice.toml`.
pub fn load_slice_manifests(root: &Path) -> (Vec<SliceManifest>, Vec<SliceManifestError>) {
    let mut crate_dirs: Vec<PathBuf> = ["apps", "games"]
        .into_iter()
        .map(|sub| root.join(sub))
        .flat_map(|dir| {
            std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect::<Vec<PathBuf>>()
        })
        .collect();
    crate_dirs.sort();

    crate_dirs
        .into_iter()
        .map(|crate_dir| crate_dir.join("slice.toml"))
        .filter(|manifest_path| manifest_path.is_file())
        .map(|manifest_path| {
            let crate_dir = manifest_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            std::fs::read_to_string(&manifest_path)
                .map_err(|e| SliceManifestError {
                    path: manifest_path,
                    message: format!("could not read file: {e}"),
                })
                .and_then(|text| parse_slice_manifest(&crate_dir, &text))
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

    #[test]
    fn parses_a_full_slice_manifest() {
        let text = r#"
            [slice]
            name = "retro-fps"
            crate_name = "axiom-game-retro-fps"
            harness_entry = "build_retro_fps_app"
            determinism_test = "retro_fps_replay_determinism"
            harness = "retro-fps"

            [[golden]]
            path = "tests/retro_fps/golden/retro_fps_state_sequence.bin"
            sha256 = "deadbeef"

            [[golden]]
            path = "tests/retro_fps/golden/retro_fps_hud_sequence.bin"
            sha256 = "cafef00d"

            [reference]
            path = "reference/retro_fps.png"
            sha256 = "abc123"
            harness = "axiom-shot"
        "#;
        let m = parse_slice_manifest(Path::new("games/retro-fps"), text).unwrap();
        assert_eq!(m.slice.name, "retro-fps");
        assert_eq!(m.slice.crate_name, "axiom-game-retro-fps");
        assert_eq!(m.slice.harness_entry, "build_retro_fps_app");
        assert_eq!(m.slice.determinism_test, "retro_fps_replay_determinism");
        assert_eq!(m.slice.harness.as_deref(), Some("retro-fps"));
        assert_eq!(m.goldens.len(), 2);
        assert_eq!(m.goldens[0].sha256, "deadbeef");
        let reference = m.reference.as_ref().expect("reference present");
        assert_eq!(reference.path, "reference/retro_fps.png");
        assert_eq!(reference.harness.as_deref(), Some("axiom-shot"));
        assert_eq!(
            m.determinism_test_path(),
            Path::new("games/retro-fps/tests/retro_fps_replay_determinism.rs")
        );
    }

    #[test]
    fn parses_a_headless_slice_without_harness_or_reference() {
        let text = r#"
            [slice]
            name = "rotating-cube-demo"
            crate_name = "axiom-demo-rotating-cube"
            harness_entry = "DemoRotatingCubeApi"
            determinism_test = "golden_artifacts"

            [[golden]]
            path = "tests/golden/full_artifact_tick0.bin"
            sha256 = "00"
        "#;
        let m = parse_slice_manifest(Path::new("apps/axiom-demo-rotating-cube"), text).unwrap();
        assert!(m.slice.harness.is_none());
        assert!(m.reference.is_none());
        assert_eq!(m.goldens.len(), 1);
    }

    #[test]
    fn unknown_field_is_rejected() {
        let text = r#"
            [slice]
            name = "x"
            crate_name = "axiom-x"
            harness_entry = "core"
            determinism_test = "t"
            mystery = true
        "#;
        assert!(parse_slice_manifest(Path::new("apps/x"), text).is_err());
    }
}
