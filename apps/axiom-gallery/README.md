# Axiom app gallery (static showcase)

A mobile-first showcase of Axiom's browser apps, published to GitHub Pages by
`.github/workflows/deploy-pages.yml` on every push to `main`.

The gallery is **not a crate**. Every app is its own standalone composition-leaf
under `apps/` with its own page and tests. This directory keeps only the static
landing grid:

```text
apps/axiom-gallery/
  web/                     # the static shell (copied verbatim into dist/)
    index.html             # landing grid: renders cards from the manifest
    gallery.js             # manifest loading + search/tag filtering (NO app list)
    styles.css             # shared dark, mobile-first styling
    home-run-css/          # standalone CSS-3D curiosity page (unlinked)
```

There is no app list in this directory. Apps register themselves with an
`app.json`; `scripts/package_gallery.py` discovers them at build time and writes
`dist/manifest.json`, which the grid fetches at runtime. Each card is a plain link
to `<id>/index.html` — no shared shell, no shared wasm bundle, no boot logic.

## Build & preview locally

`make gallery` packages the whole showcase (`scripts/package_gallery.py`): it
copies this `web/` into `dist/`, builds the shared pure-TS engine once into
`dist/engine/web-engine/<version>/`, then lays every registered app under
`dist/<id>/` — TypeScript apps bundled against the shared engine, Rust apps as
their own capability-detecting loader (`dist/<id>/axiom-loader.js` + wasm, with a
Binaryen wasm2js fallback in full mode). Finally it writes `dist/manifest.json`
and serves `dist/`:

```sh
make gallery           # package every app bundle + serve at http://localhost:8000
make gallery-fast      # quick wasm-only bundles (no wasm2js fallback) — for iteration
make gallery-serve     # re-serve the already-built dist/ WITHOUT rebuilding
make gallery-build     # package into dist/ only, no serve
```

To iterate on ONE demo with hot reload, skip packaging entirely:

```sh
cargo run -p axiom-serve -- gravix     # build + serve + auto-reload (see CLAUDE.md)
```

## Adding an app

**Apps register themselves.** An app is in the gallery if — and only if — it has
an `app.json` in its own directory. There is no list to edit here, no Makefile
target to add, and no build artifact to commit:

```sh
cargo run -p axiom-serve -- init <app>   # writes apps/<app>/app.json
```

That detects the app's kind and seeds the copy; edit the `title`, `blurb`,
`description`, and `tags`, and the card is done. `make gallery` discovers it. To
remove an app from the gallery, delete its `app.json` — or delete the app, and it
leaves with nothing left behind.

```json
{
  "title": "Axiom Arcade",
  "blurb": "One line for the card.",
  "description": "The long-form paragraph.",
  "kind": "ts-web-engine",
  "tags": ["game", "arcade"]
}
```

`kind` is `ts-web-engine` (pure TypeScript over `@axiom/web-engine`) or
`rust-wasm`. An app whose page loads files at runtime that the page itself never
references can list them under `assets`; everything the page *does* reference is
found automatically.

This replaced a scheme where each app was listed in three shared files — a
`DEMOS` array here, a Makefile target, and a committed single-file page. Because
registration lived outside the app, deleting an app left its entries behind:
five of the seven TypeScript targets had rotted into pointing at apps that no
longer existed. Ownership now sits with the app, so that class of drift is gone.

### One engine, many apps

The pure-TypeScript engine is built **once** into
`dist/engine/web-engine/<version>/`, and every TypeScript app resolves the bare
`@axiom/web-engine` specifier to it through an import map injected at package
time — the same mechanism `axiom-serve` injects in dev. Apps ship only their own
bundled code, the browser caches the engine once for the whole gallery, and an
engine fix is one rebuild rather than re-packaging every app.

Rust apps still statically link the engine into their own wasm. That is inherent
to wasm, not a choice the packager makes.

### Listing what is registered

```sh
uv run --no-project python scripts/package_gallery.py --list
```

## One-time setup

GitHub Pages must be enabled once in the repo: **Settings → Pages → Build and
deployment → Source: GitHub Actions**. After that, every push to `main` deploys.
