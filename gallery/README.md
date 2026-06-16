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

The deploy bundle is assembled into `dist/` by the same `wasm-bindgen` flow the
demos already use:

```sh
make gallery-build     # build both wasm demos + assemble dist/
make gallery           # serve dist/ at http://localhost:8000
```

Then open <http://localhost:8000/> in a WebGPU browser.

## Adding a demo

1. Build a browser/wasm app under `apps/` that exports a `start()` and binds its
   surface to a canvas id (see the two existing browser apps).
2. Add an entry to `DEMOS` in `gallery.js` (`id`, `title`, `dir`, `jsModule`,
   `canvasId`, and the `buttons` its keypad should show — empty for none).
3. Add its build + a `cp` into `dist/<dir>/pkg` to the deploy workflow and the
   `gallery-build` make recipe.

## Netplay relay

The netplay demo needs a live WebSocket relay (`tools/axiom-netcode-relay`),
which static Pages cannot host. The deployed page reads a `?relay=<url>` query
param (and shows a relay input box) so you can point it at a relay you run or
host — use `wss://` from the `https://` Pages origin. With no relay set the demo
still boots and the keypad works; it just can't connect to a peer.

## One-time setup

GitHub Pages must be enabled once in the repo: **Settings → Pages → Build and
deployment → Source: GitHub Actions**. After that, every push to `main` deploys.
