# axiom-harness — the hostile-environment harness

Takes capabilities away from the real game and shows you which rung it lands on.

The game is `apps/casino-games/web/resilient.html`, which implements one ladder,
top to bottom:

```
webgpu → webgl2 → webgl1 → canvas2d → css3d → form
```

The first five rungs are `@axiom/web-engine`'s probed render tiers. `form` is the
served document itself — nine `<button type="submit" name="pick" value="0..8">`
inside a real `<form method="POST" action="/api/pick">` — which needs no script
and no stylesheet, and is therefore the one rung nothing can fail to reach.

```sh
make hostile-harness                       # http://localhost:8091/__harness/
node tools/axiom-harness/src/main.ts --port 8091
make e2e-ladder                            # the same ladder, asserted
```

One command is enough: the harness starts the real `axiom-chest-server` on an
ephemeral loopback port and proxies to it, so the page, `POST /api/pick` and the
engine bundle all live on one origin. That is required, not tidy — a native
`<form method="POST">` navigation has no CORS story at all, so a cross-origin
harness would delete the zero-JS rung instead of testing it.

## What is in here

| file | what it is |
| --- | --- |
| `web/caps-mask.js` | **The single source of truth for denial.** Loaded verbatim by the harness (`<script src>`) and by the Playwright suite (`context.add_init_script`). |
| `web/rungs.json` | **The rung matrix.** Read by the harness UI and by the suite. |
| `web/index.html`, `harness.js`, `harness.css` | the developer UI |
| `src/main.ts` | CLI; starts the embedded chest server and the harness |
| `src/server.ts` | routes + the HTML rewrite (mask preamble, conditional import map) |
| `src/inject.ts` (+ `.test.ts`) | where the preamble goes in a document, pinned by tests |

`caps-mask.js` and `rungs.json` are shared rather than copied for one reason: a
harness whose denial differs from the suite's denial is proving a ladder nobody
ships, and a lying harness is worse than no harness.

## What can and cannot be denied — all measured, not assumed

**No browser feature removes a rendering API from a document.** There is no
`<iframe sandbox>` token, no CSP directive and no Permissions-Policy feature for
WebGPU, WebGL2, WebGL1, Canvas2D or CSS 3D. Measured: `sandbox="allow-scripts"`
still had every one of them. So the render rungs are masked *in-realm* by
`caps-mask.js`.

**Patching `iframe.contentWindow` before assigning `.src` does not work** — the
navigation installs a fresh realm and the patch is gone (measured
`injected:false`). This is why the toggles reboot the iframe with a `?deny=`
query the server understands instead of applying live, and why `srcdoc`
composition was rejected: it puts the game in an opaque origin, which breaks the
session cookie and the same-origin form POST.

**What an iframe genuinely can do** is kill JavaScript — `sandbox` *without*
`allow-scripts`. That sandbox **must keep `allow-forms`**, or the form POST is
silently swallowed and the harness deletes the exact rung the no-JS toggle exists
to prove.

**Chromium launch flags**, measured:

| flag | effect |
| --- | --- |
| `--disable-webgl2` | kills WebGL2, leaves WebGL1 alive — this *is* the webgl1 rung |
| `--disable-3d-apis` | kills WebGL2 and WebGL1 both |
| `--use-angle=swiftshader` | software-rasterized WebGL — the most Citrix-realistic condition |
| `--disable-gpu` | **does not** disable WebGL (falls back to SwiftShader). Never use it to mean "no GPU". |

No flag removes `navigator.gpu`; WebGPU denial has to be JS-level. And
`navigator.gpu` is secure-context gated — serve on `localhost`/`127.0.0.1` or the
webgpu rung silently lies.

The harness UI **says which toggles it cannot enforce** and prints the exact
`chrome …` command line for them, rather than offering a switch that would
quietly do nothing. A control that lies is worse than no control. CPU throttling
is in the same category: it is a DevTools-protocol emulation
(`Emulation.setCPUThrottlingRate`) with no in-page equivalent, so only the suite
drives it.

## The deny tokens

| token | what it denies |
| --- | --- |
| `webgpu` | `navigator.gpu` and the `GPU*` constructors |
| `webgpu-adapter` | keeps `navigator.gpu`; `requestAdapter()` resolves `null` — the faithful shape of hardware acceleration being off |
| `webgl2` / `webgl1` | those `getContext` names, plus the matching constructors |
| `canvas2d` | the `2d` context on `HTMLCanvasElement` **and** `OffscreenCanvas` |
| `css3d` | `CSS.supports("transform-style","preserve-3d")` → false, plus a `transform-style:flat !important` sheet kept last in `<head>` by a `MutationObserver` |
| `fetch` | `window.fetch` removed |
| `fetch-reject` | `fetch` present, callable, and rejecting — the condition that ships broken |
| `xhr` | `XMLHttpRequest` removed |
| `websocket`, `wasm` | those globals removed |
| `readback` | `getImageData`/`toDataURL`/`toBlob` throw `SecurityError` |

Applied tokens land on `window.__AXIOM_DENIED`; anything that would not install
lands on `window.__AXIOM_MASK_ERRORS` (the suite asserts it is empty, so a
denial that silently does nothing fails a test instead of passing one).

`webgpu` and `webgpu-adapter` are mutually exclusive by construction: one removes
the API the other needs in order to answer "no adapter".

## Reading the result

The page publishes `window.__axiomTier` (the rung it committed to, re-published
on every downgrade), `window.__renderProbe` (the engine's whole `DetectionReport`
— per-tier outcome, readback trust, elapsed ms) and `postMessage({axiomTier})` so
the harness learns it without reaching across a document boundary. The harness
*reads* that report; it never runs a detection of its own, because the page ships
its own import map and a second probe could load a different engine build and
report a tier the game never saw.

Repo tooling: outside the engine dependency graph, the Coverage Law and the
Branchless Law.
