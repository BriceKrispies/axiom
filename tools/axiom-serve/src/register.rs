//! `axiom-serve init <app>` — write an app's `app.json`, its gallery registration.
//!
//! An app joins the gallery by carrying an `app.json`; nothing central lists it.
//! This writes that file, filling in everything it can work out for itself — the
//! app's kind (from the same detection the dev server uses, so the two can never
//! disagree) and a starting title and blurb — and leaves the editorial copy for a
//! human to sharpen.
//!
//! The alternative was to derive the whole card from metadata the app already has.
//! It does not survive contact: three of the four TypeScript apps have no
//! `package.json` at all, and the one that does describes itself as "this manifest
//! exists so bare `node --test` can resolve the specifier" — accurate for npm,
//! useless on a card. A title and a blurb are editorial, so they are authored once
//! and then owned by the app.

use std::fs;
use std::path::Path;

use crate::app::AppKind;

/// The gallery's name for an app shape. Mirrors the `kind` values
/// `scripts/package_gallery.py` dispatches on.
fn gallery_kind(kind: &AppKind) -> Result<&'static str, String> {
    match kind {
        AppKind::RustWasm { .. } => Ok("rust-wasm"),
        AppKind::TsWebEngine => Ok("ts-web-engine"),
        AppKind::TsSdkHosted => Err(
            "this app is SDK-hosted (@axiom/game + the game-runtime wasm), which the gallery \
             packager does not build yet.\n       Register it once that path exists, or port the \
             app to @axiom/web-engine."
                .to_string(),
        ),
        AppKind::TsPlain => Err(
            "this app is plain TypeScript with no engine dependency, so there is nothing for the \
             gallery to resolve against.\n       Point its tsconfig at @axiom/web-engine to \
             publish it."
                .to_string(),
        ),
    }
}

/// The gallery id for an app directory: its name without the `axiom-` prefix.
/// Matches `_derive_id` in `scripts/package_gallery.py`.
fn derive_id(app_dir: &Path) -> String {
    let name = app_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("app");
    name.strip_prefix("axiom-").unwrap_or(name).to_string()
}

/// `casino-games` -> `Casino Games`. A starting point the author edits.
fn title_from_id(id: &str) -> String {
    id.split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `description = "..."` from a Cargo manifest, if there is one. Rust apps
/// carry a real prose description, which makes a far better starting blurb than a
/// placeholder.
fn cargo_description(app_dir: &Path) -> Option<String> {
    let manifest = fs::read_to_string(app_dir.join("Cargo.toml")).ok()?;
    let line = manifest
        .lines()
        .find(|line| line.trim_start().starts_with("description"))?;
    let start = line.find('"')? + 1;
    let rest = &line[start..];
    let end = rest.rfind('"')?;
    Some(rest[..end].to_string())
}

/// The first sentence of `text`, for a one-line card blurb.
fn first_sentence(text: &str) -> String {
    text.find(". ")
        .map_or_else(|| text.to_string(), |i| text[..=i].trim().to_string())
}

/// Write `apps/<app>/app.json`. Refuses to clobber an existing one unless `force`.
pub fn init(app_dir: &Path, kind: &AppKind, force: bool) -> Result<(), String> {
    let kind_name = gallery_kind(kind)?;
    let target = app_dir.join("app.json");
    if target.exists() && !force {
        return Err(format!(
            "{} already exists — the app is registered.\n       Edit it, or pass --force to \
             overwrite it with a fresh one.",
            target.display()
        ));
    }

    let id = derive_id(app_dir);
    let title = title_from_id(&id);
    let description = cargo_description(app_dir).unwrap_or_default();
    let blurb = if description.is_empty() {
        format!("TODO: one line about {title} for its gallery card.")
    } else {
        first_sentence(&description)
    };

    // Written by hand rather than through a JSON crate: axiom-serve depends on
    // tiny_http and std, and one small object is not worth widening that.
    let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let json = format!(
        "{{\n  \"title\": \"{}\",\n  \"blurb\": \"{}\",\n  \"description\": \"{}\",\n  \"kind\": \"{}\",\n  \"tags\": []\n}}\n",
        escape(&title),
        escape(&blurb),
        escape(&description),
        kind_name,
    );
    fs::write(&target, json).map_err(|e| format!("could not write {}: {e}", target.display()))?;

    println!("axiom-serve: wrote {}", target.display());
    println!("axiom-serve: registered '{id}' as {kind_name}");
    println!();
    println!("  Next: edit the title, blurb, description, and tags — they are the card's copy.");
    println!("  Then: make gallery   (or: uv run --no-project python scripts/package_gallery.py --list)");
    Ok(())
}
