# Axiom gallery e2e tests

A clean [`pytest-playwright`](https://playwright.dev/python/docs/test-runners) smoke
suite that drives the demo gallery in a real browser. **Repo tooling** — not part of
the engine dependency graph (same status as `scripts/` and the Makefile); it is not a
Cargo package and is not held to the engine's coverage/branchless/architecture laws.

## What it checks

`test_smoke.py` enters **every non-multiplayer demo** (netplay is skipped) twice — once
normally, once with `?backend=canvas2d` (the engine's runtime backend override, read by
`force_canvas2d()` in `axiom-windowing`). For each visit it asserts:

- the demo's **ready signal** appears (engine run-loop logs `axiom: render backend = …`;
  2D games log `[<id>] ready`; growth/harness signal via DOM) — a stall times out → fail;
- **no uncaught page error and no FATAL console error**. The engine logs
  `axiom: FATAL — no render backend available …` if every backend fails, so a silent
  non-render is caught. Benign noise (WebGPU `Device failed at creation` warnings, retro-fps's
  hot-reload `/event` 404 on a static server) is not fatal and is ignored;
- the **canvas actually painted** — it screenshots the canvas (saved to `screenshots/`)
  and asserts it is not a single flat color.

Under headless Chromium, WebGPU device creation fails, so the 3D demos fall back to
WebGL2 on the default pass; the `canvas2d` pass forces the software rasterizer and
asserts `render backend = Canvas2d`.

## Run

```sh
make e2e                       # builds the fast gallery, serves it, runs the suite
AXIOM_E2E_REUSE=1 make e2e     # reuse a gallery already serving on :8000 (skip rebuild)
AXIOM_PW_HEADLESS=0 make e2e   # watch it in a visible browser
```

`conftest.py` builds (`scripts/package_gallery.py --fast`) and serves `dist/` on
`:8000` for the session, then tears it down. Screenshots land in `screenshots/`
(git-ignored).
