# Axiom demo gallery

A mobile-first static gallery of Axiom's browser/WebAssembly demos, published to
GitHub Pages by `.github/workflows/deploy-pages.yml` on every push to `main`.

This directory is **repo tooling**, not part of the engine dependency graph — the
same status as the `Makefile` and `scripts/`. It is plain static HTML/CSS/JS and
declares no Cargo package, so the Layer and Module laws never see it.

## Contents

| File          | Role                                                              |
| ------------- | ----------------------------------------------------------------- |
| `index.html`  | Mobile-first landing grid of demo cards.                          |
| `demo.html`   | Shared per-demo shell (loads one demo's wasm, mounts the keypad). |
| `gallery.js`  | The demo manifest + the data-driven boot logic.                   |
| `keypad.js`   | On-screen touch keypad (dispatches synthetic arrow-key events).   |
| `styles.css`  | Shared dark, mobile-first styling.                                |

The keypad is a presentation-only shim: it dispatches synthetic
`KeyboardEvent`s on `window`, which the wasm apps already listen for, so it
drives the demos with **no changes to engine/app input code**.

## Build & preview locally

`make gallery` is the **main driver** — the one command to browse the whole
engine surface during development. It PACKAGES every browser demo
(`scripts/package_gallery.py`): each `dist/<id>/` gets a capability-detecting
loader over a wasm fast-path PLUS a Binaryen wasm2js fallback for browsers with no
WebAssembly. It then serves `dist/` locally:

```sh
make gallery           # PACKAGE all demos (wasm + wasm2js fallback) + serve at http://localhost:8000
```

Then open <http://localhost:8000/> in a WebGPU browser. Because the full packaging
rebuilds std MVP (so the wasm2js fallback is possible — see
`scripts/package_app.py`), the first `make gallery` is slow. Narrower targets back
it:

```sh
make gallery-fast      # quick wasm-only gallery (no fallback, normal incremental build) — for iteration
make gallery-serve     # re-serve the already-built dist/ WITHOUT rebuilding (fast restart)
make gallery-build     # package all demos into dist/ only, no serve
```

## Adding a demo

1. Build a browser/wasm app under `apps/` that exports a `start()` and binds its
   surface to a canvas id (see the two existing browser apps).
2. Add an entry to `DEMOS` in `gallery.js` (`id`, `title`, `dir`, `jsModule`,
   `canvasId`, and the `buttons` its keypad should show — empty for none).
3. Add an entry to `GALLERY_APPS` in `scripts/package_gallery.py` mapping the
   demo `id` to its `apps/<crate>` dir, so the packager builds it into `dist/`.
   (Shared-shell demos boot through `demo.html`; self-hosted demos set `page:` in
   the `DEMOS` manifest and own their `index.html`.)

## Netplay relay

The netplay demo needs a live WebSocket relay (`tools/axiom-netcode-relay`),
which static Pages cannot host. The deployed page reads a `?relay=<url>` query
param (and shows a relay input box) so you can point it at a relay you run or
host — use `wss://` from the `https://` Pages origin. With no relay set the demo
still boots and the keypad works; it just can't connect to a peer.

## One-time setup

GitHub Pages must be enabled once in the repo: **Settings → Pages → Build and
deployment → Source: GitHub Actions**. After that, every push to `main` deploys.
