//! `axiom-asset-pack` — the asset packer's library core (the CLI in `main.rs` is
//! a thin wrapper over [`pack`]).
//!
//! The packer is the *producer* side of the runtime asset-streaming pipeline
//! whose deterministic, I/O-free *consumer* is `modules/axiom-assets`
//! (`axiom_assets::AssetsApi`). The manifest binary format is OWNED by that
//! module; this tool encodes through `AssetsApi::encode_manifest` and verifies
//! its own output round-trips through `AssetsApi::from_manifest_bytes`, so the
//! two sides can never drift.
//!
//! ## Input format (TOML)
//!
//! ```toml
//! out_dir  = "dist"      # optional (default "dist"): output root, relative to
//!                        #   this TOML file's dir. Overridden by the CLI's
//!                        #   optional second arg (resolved relative to the CWD),
//!                        #   so authored content need not hardcode a deploy path.
//! blob_dir = "blobs"     # optional (default "blobs"): blob subdir under out_dir
//!
//! [[asset]]
//! id           = 1       # stable u64 asset id (unique, non-zero)
//! kind         = 1       # app-defined u32 kind tag (e.g. 1=mesh, 2=texture)
//! priority     = 100     # u32 streaming priority (higher loads first)
//! source       = "assets/hero.mesh"  # source file, relative to the TOML file
//! dependencies = []      # optional list of asset ids this asset depends on
//! ```
//!
//! ## Output layout
//!
//! For an input at `<dir>/pack.toml` with `out_dir = "dist"`:
//!
//! ```text
//! <dir>/dist/manifest.bin          # the Axiom-native binary manifest
//! <dir>/dist/blobs/<id>.<ext>      # one copied blob per asset, id-named
//! ```
//!
//! The manifest `locator` for each asset is the RELATIVE URL the browser fetches
//! (e.g. `"blobs/1.mesh"`) — `out_dir`-relative, forward-slashed.

use std::fmt;
use std::path::{Path, PathBuf};

use axiom_assets::AssetsApi;
use axiom_kernel::{AssetId, StableHash};

/// One manifest entry as `AssetsApi::encode_manifest` consumes it:
/// `(id, kind, priority, size, content_hash, locator, dependencies)`.
type ManifestEntry<'a> = (AssetId, u32, u32, u64, u64, &'a str, &'a [AssetId]);
use serde::Deserialize;

/// The authored asset set, deserialized from the input TOML.
#[derive(Debug, Deserialize)]
struct PackInput {
    /// Output root directory, resolved relative to the input TOML file. Optional
    /// (default "dist"); the CLI's optional second arg overrides it.
    #[serde(default = "default_out_dir")]
    out_dir: String,
    /// Blob sub-directory under `out_dir` (and the locator URL prefix).
    #[serde(default = "default_blob_dir")]
    blob_dir: String,
    /// The assets to pack, in authored order (also the manifest order).
    #[serde(default, rename = "asset")]
    assets: Vec<AssetSpec>,
}

/// One authored asset entry.
#[derive(Debug, Deserialize)]
struct AssetSpec {
    /// Stable, unique, non-zero asset id.
    id: u64,
    /// App-defined kind tag (the runtime treats it as opaque).
    kind: u32,
    /// Streaming priority; the scheduler dispatches higher values first.
    priority: u32,
    /// Source file path, resolved relative to the input TOML file.
    source: String,
    /// Ids this asset depends on (must reference assets in this same set).
    #[serde(default)]
    dependencies: Vec<u64>,
}

fn default_blob_dir() -> String {
    "blobs".to_string()
}

fn default_out_dir() -> String {
    "dist".to_string()
}

/// One packed asset, surfaced in the [`PackSummary`] for callers/tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedAsset {
    /// The asset id.
    pub id: u64,
    /// The relative locator URL written into the manifest.
    pub locator: String,
    /// The blob's byte length (the manifest `size_hint`).
    pub size: u64,
    /// The blob's deterministic content hash (the manifest `content_hash`).
    pub content_hash: u64,
}

/// The result of a successful pack run.
#[derive(Debug, Clone)]
pub struct PackSummary {
    /// Absolute path of the written `manifest.bin`.
    pub manifest_path: PathBuf,
    /// Absolute path of the blob output directory.
    pub blob_dir: PathBuf,
    /// Every packed asset, in authored order.
    pub assets: Vec<PackedAsset>,
    /// Total bytes copied across all blobs.
    pub total_bytes: u64,
}

/// Everything that can go wrong while packing.
#[derive(Debug)]
pub enum PackError {
    /// The input TOML could not be read.
    ReadInput {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The input TOML could not be parsed.
    ParseInput(toml::de::Error),
    /// A source blob could not be read.
    ReadSource {
        id: u64,
        path: PathBuf,
        source: std::io::Error,
    },
    /// An output directory or file could not be written.
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The encoded manifest failed `axiom-assets` validation (duplicate/null id,
    /// or a dependency on an unknown asset) — surfaced at pack time so a broken
    /// manifest never ships.
    InvalidManifest(String),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::ReadInput { path, source } => {
                write!(
                    f,
                    "failed to read input TOML '{}': {source}",
                    path.display()
                )
            }
            PackError::ParseInput(e) => write!(f, "failed to parse input TOML: {e}"),
            PackError::ReadSource { id, path, source } => write!(
                f,
                "asset {id}: failed to read source '{}': {source}",
                path.display()
            ),
            PackError::Write { path, source } => {
                write!(f, "failed to write '{}': {source}", path.display())
            }
            PackError::InvalidManifest(msg) => {
                write!(f, "encoded manifest is invalid: {msg}")
            }
        }
    }
}

impl std::error::Error for PackError {}

/// Pack the asset set described by the TOML file at `input_path`.
///
/// Reads each source blob (relative to the input file), copies it into
/// `out_dir/blob_dir/<id>.<ext>`, computes its size + [`StableHash`] content
/// digest, writes `out_dir/manifest.bin` via [`AssetsApi::encode_manifest`], and
/// verifies the bytes round-trip through [`AssetsApi::from_manifest_bytes`].
///
/// `out_dir_override`, when `Some`, replaces the TOML's `out_dir` and is resolved
/// relative to the current working directory (the natural meaning of a CLI path
/// argument); when `None`, the TOML's `out_dir` is used, resolved relative to the
/// input file. Source paths always resolve relative to the input file either way.
pub fn pack(input_path: &Path, out_dir_override: Option<&Path>) -> Result<PackSummary, PackError> {
    let text = std::fs::read_to_string(input_path).map_err(|source| PackError::ReadInput {
        path: input_path.to_path_buf(),
        source,
    })?;
    let input: PackInput = toml::from_str(&text).map_err(PackError::ParseInput)?;

    let base_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let out_dir = out_dir_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| base_dir.join(&input.out_dir));
    let blob_dir = out_dir.join(&input.blob_dir);
    std::fs::create_dir_all(&blob_dir).map_err(|source| PackError::Write {
        path: blob_dir.clone(),
        source,
    })?;

    // Owned `locator`/`deps` buffers must outlive the borrowing tuples
    // handed to `encode_manifest` below.
    let mut packed = Vec::with_capacity(input.assets.len());
    let mut locators = Vec::with_capacity(input.assets.len());
    let mut dep_lists = Vec::with_capacity(input.assets.len());
    let mut total_bytes: u64 = 0;

    for spec in &input.assets {
        let source_path = base_dir.join(&spec.source);
        let bytes = std::fs::read(&source_path).map_err(|source| PackError::ReadSource {
            id: spec.id,
            path: source_path.clone(),
            source,
        })?;
        let size = bytes.len() as u64;
        let content_hash = StableHash::of_bytes(&bytes).raw();

        let blob_name = id_blob_name(spec.id, &spec.source);
        let blob_path = blob_dir.join(&blob_name);
        std::fs::write(&blob_path, &bytes).map_err(|source| PackError::Write {
            path: blob_path.clone(),
            source,
        })?;

        let locator = format!("{}/{}", input.blob_dir, blob_name);

        total_bytes += size;
        packed.push(PackedAsset {
            id: spec.id,
            locator: locator.clone(),
            size,
            content_hash,
        });
        locators.push(locator);
        dep_lists.push(
            spec.dependencies
                .iter()
                .map(|&d| AssetId::from_raw(d))
                .collect::<Vec<_>>(),
        );
    }

    let entries: Vec<ManifestEntry> = input
        .assets
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            (
                AssetId::from_raw(spec.id),
                spec.kind,
                spec.priority,
                packed[i].size,
                packed[i].content_hash,
                locators[i].as_str(),
                dep_lists[i].as_slice(),
            )
        })
        .collect();

    let manifest_bytes = AssetsApi::encode_manifest(&entries);

    AssetsApi::from_manifest_bytes(&manifest_bytes, 1)
        .map_err(|e| PackError::InvalidManifest(format!("{e:?}")))?;

    let manifest_path = out_dir.join("manifest.bin");
    std::fs::write(&manifest_path, &manifest_bytes).map_err(|source| PackError::Write {
        path: manifest_path.clone(),
        source,
    })?;

    Ok(PackSummary {
        manifest_path,
        blob_dir,
        assets: packed,
        total_bytes,
    })
}

/// The id-based blob filename: `<id>` plus the source's extension, if any
/// (e.g. id 1 of `hero.mesh` -> `1.mesh`). Id-based (not content-addressed) so
/// the locator a demo author writes is predictable from the id alone.
fn id_blob_name(id: u64, source: &str) -> String {
    Path::new(source)
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| format!("{id}.{ext}"))
        .unwrap_or_else(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique scratch directory per test, under the OS temp dir.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("axiom-asset-pack-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, bytes).expect("write file");
    }

    #[test]
    fn author_pack_round_trips_through_assets_api() {
        let dir = scratch("roundtrip");
        write(&dir.join("assets/a.mesh"), b"mesh-bytes");
        write(&dir.join("assets/b.tex"), b"texture-bytes-longer");
        let toml = r#"
            out_dir = "dist"
            [[asset]]
            id = 1
            kind = 1
            priority = 100
            source = "assets/a.mesh"
            dependencies = []
            [[asset]]
            id = 2
            kind = 2
            priority = 50
            source = "assets/b.tex"
            dependencies = [1]
        "#;
        let input = dir.join("pack.toml");
        write(&input, toml.as_bytes());

        let summary = pack(&input, None).expect("pack succeeds");

        assert_eq!(summary.assets.len(), 2);
        assert_eq!(
            summary.total_bytes,
            (b"mesh-bytes".len() + b"texture-bytes-longer".len()) as u64
        );
        assert_eq!(summary.assets[0].locator, "blobs/1.mesh");
        assert_eq!(summary.assets[1].locator, "blobs/2.tex");

        assert_eq!(
            std::fs::read(dir.join("dist/blobs/1.mesh")).unwrap(),
            b"mesh-bytes"
        );
        assert_eq!(
            std::fs::read(dir.join("dist/blobs/2.tex")).unwrap(),
            b"texture-bytes-longer"
        );

        let bytes = std::fs::read(&summary.manifest_path).expect("read manifest");
        let api = AssetsApi::from_manifest_bytes(&bytes, 4).expect("manifest is valid");
        assert_eq!(api.total_count(), 2);
        assert_eq!(
            api.asset_ids(),
            vec![AssetId::from_raw(1), AssetId::from_raw(2)]
        );
        assert_eq!(api.kind(AssetId::from_raw(1)), Some(1));
        assert_eq!(api.kind(AssetId::from_raw(2)), Some(2));
        assert_eq!(
            api.locator(AssetId::from_raw(1)),
            Some("blobs/1.mesh".to_string())
        );
        assert_eq!(
            api.locator(AssetId::from_raw(2)),
            Some("blobs/2.tex".to_string())
        );
        assert_eq!(
            api.dependencies_of(AssetId::from_raw(2)),
            vec![AssetId::from_raw(1)]
        );
    }

    #[test]
    fn content_hash_matches_stable_hash_of_source_bytes() {
        let dir = scratch("hash");
        write(&dir.join("a.bin"), b"deterministic");
        let toml = r#"
            out_dir = "out"
            [[asset]]
            id = 7
            kind = 0
            priority = 1
            source = "a.bin"
        "#;
        let input = dir.join("pack.toml");
        write(&input, toml.as_bytes());

        let summary = pack(&input, None).expect("pack succeeds");
        assert_eq!(
            summary.assets[0].content_hash,
            StableHash::of_bytes(b"deterministic").raw()
        );
        assert_eq!(summary.assets[0].size, b"deterministic".len() as u64);
    }

    #[test]
    fn default_blob_dir_is_blobs_and_extensionless_sources_use_bare_id() {
        let dir = scratch("default-blobdir");
        write(&dir.join("raw"), b"x");
        let toml = r#"
            out_dir = "out"
            [[asset]]
            id = 3
            kind = 0
            priority = 0
            source = "raw"
        "#;
        let input = dir.join("pack.toml");
        write(&input, toml.as_bytes());

        let summary = pack(&input, None).expect("pack succeeds");
        assert_eq!(summary.assets[0].locator, "blobs/3");
        assert!(dir.join("out/blobs/3").exists());
    }

    #[test]
    fn missing_source_file_is_an_error() {
        let dir = scratch("missing-source");
        let toml = r#"
            out_dir = "dist"
            [[asset]]
            id = 1
            kind = 1
            priority = 1
            source = "does-not-exist.bin"
        "#;
        let input = dir.join("pack.toml");
        write(&input, toml.as_bytes());

        let err = pack(&input, None).expect_err("missing source must error");
        assert!(matches!(err, PackError::ReadSource { id: 1, .. }));
        assert!(err.to_string().contains("does-not-exist.bin"));
    }

    #[test]
    fn missing_input_file_is_an_error() {
        let dir = scratch("missing-input");
        let err = pack(&dir.join("nope.toml"), None).expect_err("missing input must error");
        assert!(matches!(err, PackError::ReadInput { .. }));
    }

    #[test]
    fn malformed_toml_is_a_parse_error() {
        let dir = scratch("bad-toml");
        let input = dir.join("pack.toml");
        write(&input, b"this is = not [valid");
        let err = pack(&input, None).expect_err("bad toml must error");
        assert!(matches!(err, PackError::ParseInput(_)));
    }

    #[test]
    fn a_dependency_on_an_unknown_asset_is_rejected_at_pack_time() {
        let dir = scratch("dangling-dep");
        write(&dir.join("a.bin"), b"a");
        let toml = r#"
            out_dir = "dist"
            [[asset]]
            id = 1
            kind = 0
            priority = 0
            source = "a.bin"
            dependencies = [999]
        "#;
        let input = dir.join("pack.toml");
        write(&input, toml.as_bytes());

        let err = pack(&input, None).expect_err("dangling dependency must error");
        assert!(matches!(err, PackError::InvalidManifest(_)));
    }

    #[test]
    fn a_duplicate_id_is_rejected_at_pack_time() {
        let dir = scratch("dup-id");
        write(&dir.join("a.bin"), b"a");
        let toml = r#"
            out_dir = "dist"
            [[asset]]
            id = 1
            kind = 0
            priority = 0
            source = "a.bin"
            [[asset]]
            id = 1
            kind = 0
            priority = 0
            source = "a.bin"
        "#;
        let input = dir.join("pack.toml");
        write(&input, toml.as_bytes());

        let err = pack(&input, None).expect_err("duplicate id must error");
        assert!(matches!(err, PackError::InvalidManifest(_)));
    }

    #[test]
    fn out_dir_override_redirects_output_and_toml_out_dir_is_optional() {
        let dir = scratch("override");
        write(&dir.join("a.bin"), b"payload");
        let toml = r#"
            [[asset]]
            id = 1
            kind = 0
            priority = 0
            source = "a.bin"
        "#;
        let input = dir.join("pack.toml");
        write(&input, toml.as_bytes());
        let target = dir.join("deploy/web");

        let summary = pack(&input, Some(&target)).expect("pack succeeds");

        assert_eq!(summary.manifest_path, target.join("manifest.bin"));
        assert!(target.join("blobs/1.bin").exists());
        assert!(!dir.join("dist").exists());
        let bytes = std::fs::read(&summary.manifest_path).expect("read manifest");
        let api = AssetsApi::from_manifest_bytes(&bytes, 1).expect("valid manifest");
        assert_eq!(
            api.locator(AssetId::from_raw(1)),
            Some("blobs/1.bin".to_string())
        );
    }
}
