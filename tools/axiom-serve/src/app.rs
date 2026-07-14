//! App resolution and shape detection.
//!
//! An "app" is a crate directory under `apps/` with a `web/` dir. Its shape
//! decides how it is built, which extra routes it is served with, what its
//! pages need injected, and which files are watched:
//!
//! - **TsSdkHosted** — TypeScript over the `@axiom/game` SDK. Served with
//!   `/vendor/axiom-game/*` (the SDK dist) and `/pkg/*` (the shared
//!   `axiom-game-runtime` wasm). Its harness already listens to `/events`.
//! - **TsWebEngine** — TypeScript over `@axiom/web-engine`. Served with
//!   `/vendor/axiom-web-engine/*`; an import map is injected into pages that
//!   lack one so the bare specifier resolves. Its harness listens to `/events`.
//! - **TsPlain** — any other TypeScript app (has `web/tsconfig.json`).
//! - **RustWasm** — a `cdylib` crate built with cargo + `wasm-bindgen` into
//!   `web/pkg/`. Pages get a full-page SSE reload script injected.
//!
//! Detection order matters: the tsconfig is checked **first** because an app
//! like `apps/axiom-game-runtime` has BOTH a `web/tsconfig.json` and a cdylib
//! `Cargo.toml` — there the TypeScript harness is the live dev loop.

use std::fs;
use std::path::{Path, PathBuf};

use crate::watch::WatchSpec;

/// The detected shape of an app (see the module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppKind {
    /// A Rust `cdylib` built with cargo + `wasm-bindgen --target web`.
    RustWasm {
        /// The cargo package name (e.g. `axiom-retro-fps`).
        crate_name: String,
        /// The artifact name (`-` → `_`, e.g. `axiom_retro_fps`).
        snake: String,
    },
    /// TypeScript over the `@axiom/game` SDK (SDK-hosted).
    TsSdkHosted,
    /// TypeScript over the pure-TS `@axiom/web-engine` package.
    TsWebEngine,
    /// Plain TypeScript (a `web/tsconfig.json` with neither SDK).
    TsPlain,
}

impl AppKind {
    /// A short human label for the startup banner.
    pub fn label(&self) -> &'static str {
        match self {
            AppKind::RustWasm { .. } => "Rust wasm (cargo + wasm-bindgen)",
            AppKind::TsSdkHosted => "TypeScript over @axiom/game (SDK-hosted)",
            AppKind::TsWebEngine => "TypeScript over @axiom/web-engine",
            AppKind::TsPlain => "plain TypeScript (tsgo)",
        }
    }
}

/// Resolve an app identifier to its crate directory, mirroring
/// `scripts/package_app.py`'s `resolve_app`: try the argument as a path, then
/// under the repo root, then under `apps/`, then with the `axiom-` prefix. A
/// candidate qualifies only if it contains a `web/` dir (this tool serves
/// browser apps). On failure the error lists every candidate tried.
pub fn resolve_app_dir(root: &Path, arg: &str) -> Result<PathBuf, String> {
    let candidates = [
        PathBuf::from(arg),
        root.join(arg),
        root.join("apps").join(arg),
        root.join("apps").join(format!("axiom-{arg}")),
    ];
    for candidate in &candidates {
        if candidate.join("web").is_dir() {
            return Ok(candidate.clone());
        }
    }
    let tried = candidates
        .iter()
        .map(|c| format!("  {}", c.display()))
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "could not find an app with a web/ dir for '{arg}'. Tried:\n{tried}"
    ))
}

/// Detect the app's shape from its on-disk files (see the module docs for the
/// detection order and why the tsconfig wins over the Cargo.toml).
pub fn detect_kind(app_dir: &Path) -> Result<AppKind, String> {
    let tsconfig = fs::read_to_string(app_dir.join("web").join("tsconfig.json")).ok();
    let cargo_toml = fs::read_to_string(app_dir.join("Cargo.toml")).ok();
    detect_kind_from(tsconfig.as_deref(), cargo_toml.as_deref())
}

/// The pure detection core: classify from the two manifest texts.
pub fn detect_kind_from(
    tsconfig: Option<&str>,
    cargo_toml: Option<&str>,
) -> Result<AppKind, String> {
    if let Some(ts) = tsconfig {
        if ts.contains("@axiom/game") {
            return Ok(AppKind::TsSdkHosted);
        }
        if ts.contains("@axiom/web-engine") {
            return Ok(AppKind::TsWebEngine);
        }
        return Ok(AppKind::TsPlain);
    }
    if let Some(toml) = cargo_toml {
        if toml.contains("cdylib") {
            let crate_name = parse_crate_name(toml)
                .ok_or("Cargo.toml has a cdylib crate-type but no [package] name")?;
            let snake = crate_name.replace('-', "_");
            return Ok(AppKind::RustWasm { crate_name, snake });
        }
    }
    Err("unrecognized app shape. Recognized shapes:\n  \
         - web/tsconfig.json (a TypeScript app: @axiom/game, @axiom/web-engine, or plain tsgo)\n  \
         - Cargo.toml with a `cdylib` crate-type next to a web/ dir (a Rust wasm app)"
        .to_string())
}

/// Parse `name = "…"` out of a Cargo.toml's `[package]` section (text scan,
/// no toml crate — the manifests here are cargo-generated and regular).
pub fn parse_crate_name(cargo_toml: &str) -> Option<String> {
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = trimmed.strip_prefix("name") {
                let rest = rest.trim_start();
                if let Some(rest) = rest.strip_prefix('=') {
                    let rest = rest.trim_start();
                    if let Some(quoted) = rest.strip_prefix('"') {
                        if let Some(end) = quoted.find('"') {
                            return Some(quoted[..end].to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// The files to watch (and what to exclude) per app shape. Build outputs are
/// excluded so a rebuild's own writes never re-trigger the watcher.
pub fn watch_spec(app_dir: &Path, kind: &AppKind) -> WatchSpec {
    let web = app_dir.join("web");
    match kind {
        AppKind::RustWasm { .. } => WatchSpec {
            roots: vec![app_dir.join("src"), app_dir.join("Cargo.toml"), web.clone()],
            exclude: vec![web.join("pkg")],
        },
        _ => WatchSpec {
            roots: vec![web.join("src"), web.join("index.html")],
            exclude: vec![web.join("dist")],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    /// A unique scratch dir per test (std only, no tempfile crate).
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "axiom-serve-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_ts_kinds_by_tsconfig_content() {
        let sdk = r#"{"paths": {"@axiom/game": ["../x"]}}"#;
        let engine = r#"{"paths": {"@axiom/web-engine": ["../x"]}}"#;
        let plain = r#"{"compilerOptions": {}}"#;
        assert_eq!(
            detect_kind_from(Some(sdk), None).unwrap(),
            AppKind::TsSdkHosted
        );
        assert_eq!(
            detect_kind_from(Some(engine), None).unwrap(),
            AppKind::TsWebEngine
        );
        assert_eq!(
            detect_kind_from(Some(plain), None).unwrap(),
            AppKind::TsPlain
        );
    }

    #[test]
    fn tsconfig_wins_over_cdylib_cargo_toml() {
        // apps/axiom-game-runtime has BOTH; the TypeScript harness is the dev loop.
        let ts = r#"{"paths": {"@axiom/game": ["../x"]}}"#;
        let cargo = "[package]\nname = \"axiom-game-runtime\"\n[lib]\ncrate-type = [\"cdylib\"]\n";
        assert_eq!(
            detect_kind_from(Some(ts), Some(cargo)).unwrap(),
            AppKind::TsSdkHosted
        );
    }

    #[test]
    fn detects_rust_wasm_with_snake_name() {
        let cargo = "[package]\nname = \"axiom-retro-fps\"\nversion = \"0.1.0\"\n\n[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n";
        assert_eq!(
            detect_kind_from(None, Some(cargo)).unwrap(),
            AppKind::RustWasm {
                crate_name: "axiom-retro-fps".to_string(),
                snake: "axiom_retro_fps".to_string(),
            }
        );
    }

    #[test]
    fn rejects_unrecognized_shapes() {
        // No tsconfig, no cdylib: not servable.
        let cargo = "[package]\nname = \"axiom-native-only\"\n";
        assert!(detect_kind_from(None, Some(cargo))
            .unwrap_err()
            .contains("Recognized shapes"));
        assert!(detect_kind_from(None, None)
            .unwrap_err()
            .contains("Recognized shapes"));
    }

    #[test]
    fn parses_package_name_not_dependency_names() {
        let cargo = "[package]\nname = \"axiom-quintet\"\n\n[dependencies]\nname = \"not-me\"\n";
        assert_eq!(parse_crate_name(cargo).as_deref(), Some("axiom-quintet"));
        // A name line before [package] (or none at all) does not count.
        assert_eq!(parse_crate_name("[dependencies]\nname = \"x\"\n"), None);
    }

    #[test]
    fn resolves_apps_by_short_name_and_lists_candidates_on_failure() {
        let root = scratch("resolve");
        fs::create_dir_all(root.join("apps").join("axiom-foo").join("web")).unwrap();

        let by_short = resolve_app_dir(&root, "foo").unwrap();
        assert!(by_short.ends_with(Path::new("apps").join("axiom-foo")));
        let by_full = resolve_app_dir(&root, "axiom-foo").unwrap();
        assert!(by_full.ends_with(Path::new("apps").join("axiom-foo")));

        let err = resolve_app_dir(&root, "definitely-not-an-app").unwrap_err();
        assert!(err.contains("definitely-not-an-app"));
        assert!(err.contains("axiom-definitely-not-an-app"));
        // All four candidate forms are listed.
        assert_eq!(
            err.lines().count(),
            5,
            "one header + four candidates:\n{err}"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_kind_over_synthetic_dirs() {
        let root = scratch("detect");
        // A web-engine TS app.
        let ts_app = root.join("ts-app");
        fs::create_dir_all(ts_app.join("web")).unwrap();
        fs::write(
            ts_app.join("web").join("tsconfig.json"),
            r#"{"paths": {"@axiom/web-engine": ["../../packages/axiom-web-engine"]}}"#,
        )
        .unwrap();
        assert_eq!(detect_kind(&ts_app).unwrap(), AppKind::TsWebEngine);

        // A Rust wasm app (no tsconfig).
        let rust_app = root.join("rust-app");
        fs::create_dir_all(rust_app.join("web")).unwrap();
        fs::write(
            rust_app.join("Cargo.toml"),
            "[package]\nname = \"axiom-demo\"\n[lib]\ncrate-type = [\"cdylib\"]\n",
        )
        .unwrap();
        assert_eq!(
            detect_kind(&rust_app).unwrap(),
            AppKind::RustWasm {
                crate_name: "axiom-demo".to_string(),
                snake: "axiom_demo".to_string(),
            }
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn watch_specs_exclude_build_outputs() {
        let app = Path::new("apps").join("axiom-x");
        let rust = watch_spec(
            &app,
            &AppKind::RustWasm {
                crate_name: "axiom-x".into(),
                snake: "axiom_x".into(),
            },
        );
        assert!(rust.roots.contains(&app.join("src")));
        assert!(rust.roots.contains(&app.join("Cargo.toml")));
        assert!(rust.roots.contains(&app.join("web")));
        assert_eq!(rust.exclude, vec![app.join("web").join("pkg")]);

        let ts = watch_spec(&app, &AppKind::TsWebEngine);
        assert!(ts.roots.contains(&app.join("web").join("src")));
        assert!(ts.roots.contains(&app.join("web").join("index.html")));
        assert_eq!(ts.exclude, vec![app.join("web").join("dist")]);
    }
}
