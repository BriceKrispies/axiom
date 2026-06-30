# Axiom demo gallery (`axiom-gallery`)

A mobile-first gallery of Axiom's browser/WebAssembly demos, published to GitHub
Pages by `.github/workflows/deploy-pages.yml` on every push to `main`.

Every browser demo is merged into **this one composition-leaf app crate**. Each
demo is a module under `src/<demo>/`; the wasm bundle exports one `<demo>_start`
entry per demo (plus a few demo-specific exports), and the static shell boots
whichever demo the user picked. There is **one wasm bundle, one loader** — not
nine.

## Layout

```text
apps/axiom-gallery/
  Cargo.toml          # the unioned manifest (every demo's deps/features)
  app.toml            # the app manifest (union of allowed layers/modules)
  src/
    lib.rs            # `pub mod <demo>;` for each demo
    <demo>/           # one demo's source (its old crate, nested as a module)
      bin/            # native, feature-gated agent drivers (retro_fps, growth)
  web/                # the WHOLE static site (served as the gallery)
    index.html        # mobile-first landing grid of demo cards
    demo.html         # shared per-demo shell (one canvas + keypad)
    gallery.js        # the demo manifest (DEMOS) + data-driven boot logic
    keypad.js         # on-screen touch keypad (synthetic arrow-key events)
    styles.css        # shared dark, mobile-first styling
    <demo>/index.html # each demo's standalone/self-hosted page
  examples/           # native dev utilities (growth worldgen benches/renders)
  tests/              # every demo's integration tests (namespaced per demo)
```

The merge rules (so nine independent crates coexist in one):

- **Entries are namespaced.** Every demo's wasm entry was `start`; in one crate
  the exports must be globally unique, so each is `<demo>_start`
  (`retro_fps_start`, `quintet_start`, …). Growth has no `start` (it exports
  `generate`/`descend`/…). Every other export was already unique and is unchanged.
- **`crate::` is rebound** to `crate::<demo>::` inside each nested demo.
- **Native agent drivers survive as feature-gated bins**: `retro-fps-agent`
  (feature `retro-fps-agent`), `growth-agent` (feature `growth-agent`), and the
  physics report runner `physics-crucible-report`.

The keypad is a presentation-only shim: it dispatches synthetic `KeyboardEvent`s
on `window`, which the wasm demos already listen for, so it drives them with **no
changes to engine/app input code**.

## Build & preview locally

`make gallery` is the **main driver** — the one command to browse the whole engine
surface. It packages the single bundle (`scripts/package_gallery.py`): one
capability-detecting loader (`dist/axiom-loader.js`) over a wasm fast-path PLUS a
Binaryen wasm2js fallback for browsers with no WebAssembly, with the static site
laid over it. It then serves `dist/` locally:

```sh
make gallery           # PACKAGE the bundle (wasm + wasm2js fallback) + serve at http://localhost:8000
```

Then open <http://localhost:8000/> in a WebGPU browser. Because the full packaging
rebuilds std MVP (so the wasm2js fallback is possible — see
`scripts/package_app.py`), the first `make gallery` is slow. Narrower targets back
it:

```sh
make gallery-fast      # quick wasm-only bundle (no fallback, normal incremental build) — for iteration
make gallery-serve     # re-serve the already-built dist/ WITHOUT rebuilding (fast restart)
make gallery-build     # package the bundle into dist/ only, no serve
```

Every page imports the one loader and calls its demo's entry: the shell does
`import("./axiom-loader.js") -> default() -> <demo>_start()`; a self-hosted page
imports `../axiom-loader.js`.

## Adding a demo

1. Add the demo's source as a module under `src/<demo>/` and declare it in
   `src/lib.rs` (`pub mod <demo>;`). Give its wasm entry a unique name
   (`#[wasm_bindgen] pub fn <demo>_start()`), and add any deps/features it needs to
   the unioned `Cargo.toml` and the allowlists in `app.toml`.
2. Add its page at `web/<demo>/index.html` (importing `../axiom-loader.js` and
   calling `<demo>_start`).
3. Add an entry to `DEMOS` in `web/gallery.js`: a shared-shell demo sets
   `startFn`, `canvasId`, and its keypad `buttons`; a self-hosted demo sets `page`
   and owns its `web/<demo>/index.html`.

No `package_gallery.py` change is needed — it builds the one crate and copies the
whole `web/` site.

## Netplay

The netplay demo needs a live authoritative server (`make netplay-dotnet`, or the
Rust `make netplay-server`), which static Pages cannot host. The deployed page
reads `?relay=`/`?server=` so you can point it at a server you run. With no server
set the demo still boots and the keypad works; it just can't connect to a peer.

## One-time setup

GitHub Pages must be enabled once in the repo: **Settings → Pages → Build and
deployment → Source: GitHub Actions**. After that, every push to `main` deploys.
