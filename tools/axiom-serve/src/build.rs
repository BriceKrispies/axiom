//! Build commands and one-time prerequisites, per app shape.
//!
//! - **RustWasm** — `cargo build -p <crate> --target wasm32-unknown-unknown`
//!   (release, or debug with `--debug`), then `wasm-bindgen --target web` into
//!   the app's `web/pkg/`. A startup preflight pins the installed
//!   `wasm-bindgen` CLI to the `wasm-bindgen` crate version in `Cargo.lock`
//!   (the two must match or the generated glue is rejected at runtime).
//! - **TS kinds** — `tsgo -p <app>/web/tsconfig.json`, the same compiler the
//!   Makefile and `scripts/axiom_dev_server.mjs` use, borrowed from
//!   `packages/axiom-game`'s node_modules (a build-time toolchain, exactly as
//!   `scripts/package_gallery.py` borrows it). One-time
//!   prerequisites run only when their outputs are missing: the `@axiom/game`
//!   dist (TsSdkHosted), the shared `axiom-game-runtime` wasm pkg
//!   (TsSdkHosted), and the `@axiom/web-engine` dist (TsWebEngine).
//!
//! All children stream stdio to the terminal (inherited), so compiler errors
//! land where the developer is looking.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::app::AppKind;

/// The wasm target triple every browser app builds for.
const WASM_TARGET: &str = "wasm32-unknown-unknown";

/// Everything needed to (re)build one app. Cloned into the watcher thread.
#[derive(Clone)]
pub struct BuildPlan {
    pub root: PathBuf,
    pub app_dir: PathBuf,
    pub kind: AppKind,
    pub debug: bool,
}

impl BuildPlan {
    /// One-time startup work: toolchain preflights and missing-output
    /// prerequisites. Failures here are fatal (the app cannot build at all).
    pub fn prepare(&self) -> Result<(), String> {
        match &self.kind {
            AppKind::RustWasm { .. } => preflight_wasm_bindgen(&self.root),
            AppKind::TsSdkHosted => {
                self.ensure_tsgo()?;
                self.ensure_game_sdk_dist()?;
                self.ensure_game_runtime_pkg()
            }
            AppKind::TsWebEngine => {
                self.ensure_tsgo()?;
                self.ensure_web_engine_dist()
            }
            AppKind::TsPlain => self.ensure_tsgo(),
        }
    }

    /// Build the app once. Called for the initial build and on every change.
    pub fn build(&self) -> Result<(), String> {
        match &self.kind {
            AppKind::RustWasm { crate_name, snake } => {
                let mut cargo = Command::new("cargo");
                cargo
                    .arg("build")
                    .arg("-p")
                    .arg(crate_name)
                    .args(["--target", WASM_TARGET]);
                if !self.debug {
                    cargo.arg("--release");
                }
                cargo.current_dir(&self.root);
                let profile_flag = if self.debug { "" } else { " --release" };
                run(
                    &mut cargo,
                    &format!("cargo build -p {crate_name} --target {WASM_TARGET}{profile_flag}"),
                )?;

                let profile = if self.debug { "debug" } else { "release" };
                let wasm = self
                    .root
                    .join("target")
                    .join(WASM_TARGET)
                    .join(profile)
                    .join(format!("{snake}.wasm"));
                let out_dir = self.app_dir.join("web").join("pkg");
                wasm_bindgen(&self.root, &wasm, &out_dir)
            }
            _ => {
                let web = self.app_dir.join("web");
                let tsconfig = web.join("tsconfig.json");
                let mut tsgo = tsgo_command(&self.root);
                tsgo.arg("-p").arg(&tsconfig).current_dir(&web);
                run(&mut tsgo, &format!("tsgo -p {}", tsconfig.display()))
            }
        }
    }

    /// The tsgo compiler is a devDependency of `packages/axiom-game`; install
    /// that package's node_modules if the binary is missing (the same
    /// toolchain-bootstrap step every `gallery-*` Makefile target runs).
    fn ensure_tsgo(&self) -> Result<(), String> {
        if tsgo_path(&self.root).is_file() {
            return Ok(());
        }
        println!("axiom-serve: tsgo missing — installing packages/axiom-game node_modules (once)");
        npm(
            &self.root.join("packages").join("axiom-game"),
            &["install", "--no-audit", "--no-fund"],
        )
    }

    /// TsSdkHosted pages import the SDK from `/vendor/axiom-game/` — build its
    /// dist once if missing.
    fn ensure_game_sdk_dist(&self) -> Result<(), String> {
        let sdk = self.root.join("packages").join("axiom-game");
        if sdk.join("dist").join("index.js").is_file() {
            return Ok(());
        }
        println!("axiom-serve: @axiom/game dist missing — building it (once)");
        npm(&sdk, &["install", "--no-audit", "--no-fund"])?;
        npm(&sdk, &["run", "build"])
    }

    /// TsWebEngine pages import the engine from `/vendor/axiom-web-engine/` —
    /// build its dist once if missing.
    fn ensure_web_engine_dist(&self) -> Result<(), String> {
        let engine = self.root.join("packages").join("axiom-web-engine");
        if engine.join("dist").join("index.js").is_file() {
            return Ok(());
        }
        println!("axiom-serve: @axiom/web-engine dist missing — building it (once)");
        npm(&engine, &["install", "--no-audit", "--no-fund"])?;
        npm(&engine, &["run", "build"])
    }

    /// TsSdkHosted pages load the shared game-agnostic wasm engine from
    /// `/pkg/` — build `axiom-game-runtime`'s pkg once if missing.
    fn ensure_game_runtime_pkg(&self) -> Result<(), String> {
        let pkg = self
            .root
            .join("apps")
            .join("axiom-game-runtime")
            .join("web")
            .join("pkg");
        if pkg.join("axiom_game_runtime.js").is_file() {
            return Ok(());
        }
        println!("axiom-serve: axiom-game-runtime wasm pkg missing — building it (once)");
        preflight_wasm_bindgen(&self.root)?;
        let mut cargo = Command::new("cargo");
        cargo
            .args([
                "build",
                "-p",
                "axiom-game-runtime",
                "--target",
                WASM_TARGET,
                "--release",
            ])
            .current_dir(&self.root);
        run(
            &mut cargo,
            &format!("cargo build -p axiom-game-runtime --target {WASM_TARGET} --release"),
        )?;
        let wasm = self
            .root
            .join("target")
            .join(WASM_TARGET)
            .join("release")
            .join("axiom_game_runtime.wasm");
        wasm_bindgen(&self.root, &wasm, &pkg)
    }
}

/// Run a prepared command with inherited stdio, mapping non-success to an error.
fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    println!("axiom-serve: $ {what}");
    let status = cmd
        .status()
        .map_err(|err| format!("could not start `{what}`: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{what}` failed ({status})"))
    }
}

/// `wasm-bindgen --target web --out-dir <out_dir> <wasm>`.
fn wasm_bindgen(root: &Path, wasm: &Path, out_dir: &Path) -> Result<(), String> {
    let mut cmd = Command::new("wasm-bindgen");
    cmd.args(["--target", "web", "--out-dir"])
        .arg(out_dir)
        .arg(wasm)
        .current_dir(root);
    run(
        &mut cmd,
        &format!(
            "wasm-bindgen --target web --out-dir {} {}",
            out_dir.display(),
            wasm.display()
        ),
    )
}

/// The tsgo binary vendored by `packages/axiom-game`'s node_modules.
fn tsgo_path(root: &Path) -> PathBuf {
    let bin = if cfg!(windows) { "tsgo.cmd" } else { "tsgo" };
    root.join("packages")
        .join("axiom-game")
        .join("node_modules")
        .join(".bin")
        .join(bin)
}

/// A `tsgo` invocation. On Windows the binary is a `.cmd` shim, which must be
/// launched through `cmd /C`; elsewhere it runs directly.
fn tsgo_command(root: &Path) -> Command {
    let tsgo = tsgo_path(root);
    if cfg!(windows) {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(tsgo);
        cmd
    } else {
        Command::new(tsgo)
    }
}

/// Run `npm --prefix <package_dir> <args…>` (via `cmd /C` on Windows, where
/// npm is itself a `.cmd` shim).
fn npm(package_dir: &Path, args: &[&str]) -> Result<(), String> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg("npm");
        c
    } else {
        Command::new("npm")
    };
    cmd.arg("--prefix").arg(package_dir).args(args);
    run(
        &mut cmd,
        &format!("npm --prefix {} {}", package_dir.display(), args.join(" ")),
    )
}

/// Verify the installed `wasm-bindgen` CLI matches the `wasm-bindgen` crate
/// version pinned in `Cargo.lock`. A mismatch produces broken glue at runtime,
/// so it is caught at startup with the exact install command to run.
pub fn preflight_wasm_bindgen(root: &Path) -> Result<(), String> {
    let lock = fs::read_to_string(root.join("Cargo.lock"))
        .map_err(|err| format!("could not read Cargo.lock at the repo root: {err}"))?;
    let want = parse_wasm_bindgen_lock_version(&lock)
        .ok_or("Cargo.lock has no `wasm-bindgen` package entry")?;
    let have = Command::new("wasm-bindgen")
        .arg("--version")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| parse_wasm_bindgen_cli_version(&String::from_utf8_lossy(&out.stdout)));
    let fix = format!("cargo install wasm-bindgen-cli --version {want} --locked");
    match have {
        Some(v) if v == want => Ok(()),
        Some(v) => Err(format!(
            "wasm-bindgen CLI {v} does not match Cargo.lock's wasm-bindgen {want}\n  fix with: {fix}"
        )),
        None => Err(format!("wasm-bindgen CLI not found on PATH\n  install with: {fix}")),
    }
}

/// Text-scan `Cargo.lock` for the `wasm-bindgen` package's version: find the
/// exact `name = "wasm-bindgen"` line, then the next `version = "…"` line
/// within the same `[[package]]` block. Exact-match on the quoted name keeps
/// `wasm-bindgen-backend`/`-shared`/`-macro` from matching.
pub fn parse_wasm_bindgen_lock_version(lock: &str) -> Option<String> {
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line.trim() != r#"name = "wasm-bindgen""# {
            continue;
        }
        for follow in lines.by_ref() {
            let trimmed = follow.trim();
            if let Some(rest) = trimmed.strip_prefix("version = \"") {
                return rest.split('"').next().map(str::to_string);
            }
            if trimmed.starts_with("[[") {
                break;
            }
        }
        return None;
    }
    None
}

/// Parse `wasm-bindgen 0.2.x` (the CLI's `--version` output) to `0.2.x`.
pub fn parse_wasm_bindgen_cli_version(stdout: &str) -> Option<String> {
    stdout.split_whitespace().nth(1).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_scan_finds_exactly_the_wasm_bindgen_package() {
        let lock = "\
[[package]]
name = \"wasm-bindgen-backend\"
version = \"0.2.99\"

[[package]]
name = \"wasm-bindgen\"
version = \"0.2.104\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"

[[package]]
name = \"wasm-bindgen-macro\"
version = \"0.2.104\"
";
        assert_eq!(
            parse_wasm_bindgen_lock_version(lock).as_deref(),
            Some("0.2.104")
        );
    }

    #[test]
    fn lock_scan_handles_missing_entry_and_missing_version() {
        assert_eq!(parse_wasm_bindgen_lock_version(""), None);
        let only_others = "[[package]]\nname = \"wasm-bindgen-shared\"\nversion = \"0.2.1\"\n";
        assert_eq!(parse_wasm_bindgen_lock_version(only_others), None);
        // A name line whose block ends before any version line.
        let truncated = "[[package]]\nname = \"wasm-bindgen\"\n\n[[package]]\nname = \"x\"\nversion = \"1.0.0\"\n";
        assert_eq!(parse_wasm_bindgen_lock_version(truncated), None);
    }

    #[test]
    fn cli_version_parses_the_second_token() {
        assert_eq!(
            parse_wasm_bindgen_cli_version("wasm-bindgen 0.2.104\n").as_deref(),
            Some("0.2.104")
        );
        assert_eq!(parse_wasm_bindgen_cli_version(""), None);
    }

    #[test]
    fn tsgo_path_points_into_the_game_sdk_toolchain() {
        let p = tsgo_path(Path::new("root"));
        assert!(p.starts_with(Path::new("root").join("packages").join("axiom-game")));
        let name = p.file_name().unwrap().to_str().unwrap();
        assert!(name == "tsgo" || name == "tsgo.cmd");
    }
}
