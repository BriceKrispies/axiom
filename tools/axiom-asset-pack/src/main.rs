//! `axiom-asset-pack` — native CLI that packs an authored TOML asset set into the
//! engine's Axiom-native binary manifest (`manifest.bin`) plus served blobs.
//!
//! Usage:
//!   cargo run -p axiom-asset-pack -- <input.toml> [out-dir]
//!
//! `out-dir` (optional) overrides the TOML's `out_dir` and is resolved relative to
//! the current working directory — so the same authored set can be packed to any
//! deploy target (e.g. an app's `web/` dir) without editing the input.
//!
//! The input format and output layout are documented on [`axiom_asset_pack::pack`]
//! and in `tools/axiom-asset-pack/README.md`. The CLI is a thin wrapper: it
//! resolves the input path, runs the packer, and prints a one-screen summary.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let input = match args.next() {
        Some(path) => PathBuf::from(path),
        None => {
            eprintln!("usage: axiom-asset-pack <input.toml> [out-dir]");
            return ExitCode::FAILURE;
        }
    };
    let out_dir_override = args.next().map(PathBuf::from);

    match axiom_asset_pack::pack(&input, out_dir_override.as_deref()) {
        Ok(summary) => {
            println!("axiom-asset-pack: packed {} asset(s)", summary.assets.len());
            for asset in &summary.assets {
                println!(
                    "  id {:<6} {:>10} bytes  hash {:#018x}  -> {}",
                    asset.id, asset.size, asset.content_hash, asset.locator
                );
            }
            println!("  total blob bytes: {}", summary.total_bytes);
            println!("  blobs:    {}", summary.blob_dir.display());
            println!("  manifest: {}", summary.manifest_path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("axiom-asset-pack: error: {e}");
            ExitCode::FAILURE
        }
    }
}
