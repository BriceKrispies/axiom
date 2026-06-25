# Axiom — repo tooling Makefile.
#
# This is repo tooling (alongside scripts/), NOT part of the engine
# dependency graph. It declares no package and is invisible to the Layer,
# Module, and App laws — same status as the xtask crate and the coverage
# scripts.
#
# Primary target: `make demo` serves the browser-visible rotating-cube slice
# (apps/axiom-demo-rotating-cube-browser) over http://localhost using uv to
# run Python's static file server. WebGPU requires an http:// origin, so a
# plain file:// open will not work.

BROWSER_DEMO_DIR := apps/axiom-demo-rotating-cube-browser
BROWSER_CRATE    := axiom-demo-rotating-cube-browser
WASM_TARGET      := wasm32-unknown-unknown
WASM_ARTIFACT    := target/$(WASM_TARGET)/release/axiom_demo_rotating_cube_browser.wasm
WEB_DIR          := $(BROWSER_DEMO_DIR)/web
PKG_DIR          := $(WEB_DIR)/pkg
PORT             ?= 8000

# The live 2-browser SERVER-AUTHORITATIVE multiplayer demo
# (apps/axiom-netplay-browser + the tools/axiom-netplay-server authoritative
# server). The browser networking is the TypeScript @axiom/client SDK
# (packages/axiom-client), built and vendored into the page by netplay-build.
NETPLAY_DIR      := apps/axiom-netplay-browser
NETPLAY_CRATE    := axiom-netplay-browser
NETPLAY_ARTIFACT := target/$(WASM_TARGET)/release/axiom_netplay_browser.wasm
NETPLAY_WEB      := $(NETPLAY_DIR)/web
NETPLAY_PKG      := $(NETPLAY_WEB)/pkg
NETPLAY_PORT     ?= 8000

# The DOOM-style first-person demo (apps/axiom-doom-browser).
DOOM_DIR         := apps/axiom-doom-browser
DOOM_CRATE       := axiom-doom-browser
DOOM_ARTIFACT    := target/$(WASM_TARGET)/release/axiom_doom_browser.wasm
DOOM_WEB         := $(DOOM_DIR)/web
DOOM_PKG         := $(DOOM_WEB)/pkg
DOOM_PORT        ?= 8000

# The walkable Growth procedural-terrain viewer (apps/axiom-growth).
GROWTH_DIR       := apps/axiom-growth
GROWTH_CRATE     := axiom-growth
GROWTH_ARTIFACT  := target/$(WASM_TARGET)/release/axiom_growth.wasm
GROWTH_WEB       := $(GROWTH_DIR)/web
GROWTH_PKG       := $(GROWTH_WEB)/pkg
GROWTH_PORT      ?= 8000

# The N-spinning-cubes load/stress visual (apps/axiom-stress-cubes-browser).
STRESS_DIR       := apps/axiom-stress-cubes-browser
STRESS_CRATE     := axiom-stress-cubes-browser
STRESS_ARTIFACT  := target/$(WASM_TARGET)/release/axiom_stress_cubes_browser.wasm
STRESS_WEB       := $(STRESS_DIR)/web
STRESS_PKG       := $(STRESS_WEB)/pkg
STRESS_PORT      ?= 8000

# The roomed-puzzle editor/playtest browser app (apps/axiom-roomed-puzzle).
ROOMED_DIR       := apps/axiom-roomed-puzzle
ROOMED_CRATE     := axiom-roomed-puzzle
ROOMED_ARTIFACT  := target/$(WASM_TARGET)/release/axiom_roomed_puzzle.wasm
ROOMED_WEB       := $(ROOMED_DIR)/web
ROOMED_PKG       := $(ROOMED_WEB)/pkg

# The Quintet block-placement browser game (apps/axiom-quintet).
QUINTET_DIR      := apps/axiom-quintet
QUINTET_CRATE    := axiom-quintet
QUINTET_ARTIFACT := target/$(WASM_TARGET)/release/axiom_quintet.wasm
QUINTET_WEB      := $(QUINTET_DIR)/web
QUINTET_PKG      := $(QUINTET_WEB)/pkg

# The browser debug-overlay developer harness (apps/axiom-browser-dev-harness).
HARNESS_DIR      := apps/axiom-browser-dev-harness
HARNESS_CRATE    := axiom-browser-dev-harness
HARNESS_ARTIFACT := target/$(WASM_TARGET)/release/axiom_browser_dev_harness.wasm
HARNESS_WEB      := $(HARNESS_DIR)/web
HARNESS_PKG      := $(HARNESS_WEB)/pkg
HARNESS_PORT     ?= 8000

GALLERY_DIR      := gallery
DIST_DIR         := dist
GALLERY_PORT     ?= 8000

.PHONY: demo demo-build netplay netplay-build netplay-server netplay-dotnet relay doom doom-build doom-hot stress stress-build growth growth-build harness harness-build agent agent-render agent-bridge gallery gallery-build gallery-serve package ts-gate help

help:
	@echo "Axiom tooling targets:"
	@echo ""
	@echo "  ===> MAIN DRIVER — the demo gallery (build everything + serve locally):"
	@echo "  make gallery        Build EVERY browser demo, assemble dist/, and serve at http://localhost:$(GALLERY_PORT)"
	@echo "  make gallery-serve  Re-serve the already-built dist/ WITHOUT rebuilding (fast restart)"
	@echo "  make gallery-build  Build all demos + assemble dist/ only, no serve (what deploy-pages.yml runs)"
	@echo "  make GALLERY_PORT=9000 gallery   Serve on a different port"
	@echo "  (this is the one command to browse the whole engine surface; the per-demo"
	@echo "   targets below are for iterating on a single app in isolation.)"
	@echo ""
	@echo "  Individual demos (the gallery already builds all of these):"
	@echo "  make demo          Serve the browser rotating-cube slice at http://localhost:$(PORT) (uses uv)"
	@echo "  make demo-build    Rebuild the rotating-cube wasm bundle into web/pkg"
	@echo "  make PORT=9000 demo   Serve on a different port"
	@echo ""
	@echo "  Live 2-browser SERVER-AUTHORITATIVE multiplayer demo:"
	@echo "  make netplay-build   Rebuild the netplay wasm bundle + vendor the @axiom/client SDK"
	@echo "  make netplay-dotnet  Run the .NET 10 server: serves the client AND the game at http://localhost:8090"
	@echo "  (run 'make netplay-build' once, then 'make netplay-dotnet' and open"
	@echo "   http://localhost:8090 in TWO WebGPU browsers — one server does it all.)"
	@echo ""
	@echo "  Alternative (Rust server + separate static serve):"
	@echo "  make netplay-server Run the Rust authoritative server (ws://127.0.0.1:9002)"
	@echo "  make netplay        Serve the page at http://localhost:$(NETPLAY_PORT)"
	@echo "  (then open http://localhost:$(NETPLAY_PORT)/?server=ws://127.0.0.1:9002 in two browsers.)"
	@echo ""
	@echo "  DOOM-style first-person demo:"
	@echo "  make doom-build    Rebuild the doom wasm bundle into its web/pkg"
	@echo "  make doom          Serve the doom page at http://localhost:$(DOOM_PORT)"
	@echo "  make doom-hot      Serve doom with live level hot-reload at http://localhost:8080"
	@echo ""
	@echo "  Load/stress visual (N spinning cubes):"
	@echo "  make stress-build  Rebuild the stress wasm bundle into its web/pkg"
	@echo "  make stress        Serve the stress page at http://localhost:$(STRESS_PORT)"
	@echo "  (open with ?cubes=N, or click the presets, to change the cube count.)"
	@echo ""
	@echo "  Browser debug-overlay developer harness:"
	@echo "  make harness-build Rebuild the harness wasm bundle into its web/pkg"
	@echo "  make harness       Serve the harness page at http://localhost:$(HARNESS_PORT)"
	@echo "  (press the backquote key in the page to open the debug overlay.)"
	@echo ""
	@echo "  Package ONE game into a self-contained, droppable bundle (wasm + wasm2js fallback):"
	@echo "  make package APP=quintet           Build dist-app/quintet/ (loader picks wasm or wasm2js)"
	@echo "  make package APP=quintet INLINE=1  Single self-contained index.html"
	@echo "  (needs a nightly toolchain with rust-src; first build rebuilds std and is slow.)"
	@echo ""
	@echo "  TypeScript SDK gate (@axiom/client static-analysis/branchless/coverage laws):"
	@echo "  make ts-gate       Run tsgo typecheck + Oxlint + 100% coverage for packages/axiom-client"

# Serve the prebuilt wasm bundle. uv provides/manages the Python interpreter;
# --no-project keeps it from trying to sync a Python project in the repo root.
# Recipe lines are kept portable (no sh-only test/||/{}) so make runs them under
# cmd.exe on Windows too; run `make demo-build` first if the page is blank.
demo:
	@echo Serving rotating-cube demo at http://localhost:$(PORT) - run make demo-build first if blank
	@echo Open it in a WebGPU browser such as recent Chrome or Edge. Ctrl+C to stop.
	uv run --no-project python -m http.server $(PORT) --directory $(WEB_DIR)

# Rebuild the wasm bundle from the browser app crate into web/pkg. Uses the
# raw toolchain (cargo + wasm-bindgen) rather than wasm-pack so the binding
# generator is the exact wasm-bindgen version locked in Cargo.lock — no
# separately-downloaded copy that can drift. Requires:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <matches Cargo.lock>
demo-build:
	cargo build -p $(BROWSER_CRATE) --target $(WASM_TARGET) --release
	wasm-bindgen --target web --out-dir $(PKG_DIR) $(WASM_ARTIFACT)

# --- Live 2-browser SERVER-AUTHORITATIVE multiplayer demo ---

# The authoritative game server: holds the state, accepts JoinRoom/ClientIntent,
# and broadcasts ServerSnapshots over the axiom-net-protocol wire format. Run
# this first, in its own shell; leave it running.
netplay-server:
	cargo run -p axiom-netplay-server

# The .NET 10 example server (examples/axiom-netplay-dotnet): an all-in-one host
# that SERVES the client (the built web/ dir) AND is the authoritative game
# server on the same origin (WebSocket at /ws), speaking the axiom-net-protocol
# wire format via a C# twin of the codec. Run `make netplay-build` first so the
# wasm bundle + vendored SDK exist, then open http://localhost:8090.
netplay-dotnet:
	dotnet run --project examples/axiom-netplay-dotnet

# The dumb lockstep broadcast relay (legacy tooling; the netplay demo no longer
# uses it, but the tool is kept for lockstep experiments).
relay:
	cargo run -p axiom-netcode-relay

# Rebuild the netplay wasm bundle (same raw cargo + wasm-bindgen flow as the
# rotating-cube demo) AND build + vendor the TypeScript @axiom/client SDK the
# page uses for networking (compiled to ESM into web/vendor/axiom-client).
netplay-build:
	cargo build -p $(NETPLAY_CRATE) --target $(WASM_TARGET) --release
	wasm-bindgen --target web --out-dir $(NETPLAY_PKG) $(NETPLAY_ARTIFACT)
	npm --prefix packages/axiom-client install --no-audit --no-fund
	npm --prefix packages/axiom-client run build
	uv run --no-project python -c "import shutil, pathlib; d = pathlib.Path('$(NETPLAY_WEB)/vendor/axiom-client'); shutil.rmtree(d, ignore_errors=True); d.parent.mkdir(parents=True, exist_ok=True); shutil.copytree('packages/axiom-client/dist', d)"
	cargo build -p axiom-netplay-ffi --release

# Serve the netplay page. The authoritative server (make netplay-server) must
# already be running, then open this URL in TWO WebGPU browser windows. Recipe
# lines are kept portable (no sh-only test/||/{}) so make runs them under cmd.exe
# on Windows too; if the bundle is missing the page reports it, so run
# `make netplay-build` first.
netplay:
	@echo Serving netplay at http://localhost:$(NETPLAY_PORT) - run make netplay-build first if blank
	@echo Start the authoritative server in another shell with:  make netplay-server
	@echo Then open this URL in TWO WebGPU browser windows and arrow-key your cube.
	uv run --no-project python -m http.server $(NETPLAY_PORT) --directory $(NETPLAY_WEB)

# --- DOOM-style first-person demo ---

# Rebuild the doom wasm bundle (same raw cargo + wasm-bindgen flow).
doom-build:
	cargo build -p $(DOOM_CRATE) --target $(WASM_TARGET) --release
	wasm-bindgen --target web --out-dir $(DOOM_PKG) $(DOOM_ARTIFACT)

# Serve the doom page. Run `make doom-build` first if blank. Open in a WebGPU
# browser; tank controls (arrows/WASD + Space).
doom:
	@echo Serving doom at http://localhost:$(DOOM_PORT) - run make doom-build first if blank
	@echo Open it in a WebGPU browser. Tank controls: arrows/WASD to move+turn, Space to fire.
	uv run --no-project python -m http.server $(DOOM_PORT) --directory $(DOOM_WEB)

# Serve the doom page with LIVE LEVEL HOT-RELOAD. The axiom-dev-reload dev server
# serves web/ (like `make doom` does) and additionally watches level.axiom,
# pushing every saved edit to the browser over SSE — edit a wall and watch it
# update with no recompile and no reload. Run `make doom-build` first; then open
# http://localhost:8080 and edit apps/axiom-doom-browser/level.axiom.
doom-hot:
	@echo Serving doom with hot-reload at http://localhost:8080 - run make doom-build first if blank
	@echo Edit apps/axiom-doom-browser/level.axiom and save to reload the level live.
	cargo run -p axiom-dev-reload

# --- Load/stress visual (N spinning cubes) ---

# Rebuild the stress wasm bundle (same raw cargo + wasm-bindgen flow).
stress-build:
	cargo build -p $(STRESS_CRATE) --target $(WASM_TARGET) --release
	wasm-bindgen --target web --out-dir $(STRESS_PKG) $(STRESS_ARTIFACT)

# Serve the stress page. Run `make stress-build` first if blank. Open in a
# WebGPU browser; change the cube count with ?cubes=N or the on-page presets.
stress:
	@echo Serving stress visual at http://localhost:$(STRESS_PORT) - run make stress-build first if blank
	@echo Open it in a WebGPU browser. Change cube count with ?cubes=N or the presets.
	uv run --no-project python -m http.server $(STRESS_PORT) --directory $(STRESS_WEB)

# --- Growth: the walkable procedural-terrain viewer (apps/axiom-growth) ---

# Rebuild the Growth viewer wasm bundle (same raw cargo + wasm-bindgen flow).
growth-build:
	cargo build -p $(GROWTH_CRATE) --target $(WASM_TARGET) --release
	wasm-bindgen --target web --out-dir $(GROWTH_PKG) $(GROWTH_ARTIFACT)

# Serve the Growth terrain viewer. Run `make growth-build` first if blank. Open
# in a WebGPU browser; click the canvas to capture the mouse, then WASD + mouse
# to walk around the generated terrain.
growth:
	@echo Serving Growth terrain viewer at http://localhost:$(GROWTH_PORT) - run make growth-build first if blank
	@echo Open it in a WebGPU browser. Click to capture the mouse, WASD to move, mouse to look.
	uv run --no-project python -m http.server $(GROWTH_PORT) --directory $(GROWTH_WEB)

# --- Browser debug-overlay developer harness (apps/axiom-browser-dev-harness) ---

# Rebuild the harness wasm bundle (same raw cargo + wasm-bindgen flow).
harness-build:
	cargo build -p $(HARNESS_CRATE) --target $(WASM_TARGET) --release
	wasm-bindgen --target web --out-dir $(HARNESS_PKG) $(HARNESS_ARTIFACT)

# Serve the harness page. Run `make harness-build` first if blank. Open in any
# browser and press the backquote (`) key to toggle the debug overlay; type
# `help` in its console.
harness:
	@echo Serving debug-overlay harness at http://localhost:$(HARNESS_PORT) - run make harness-build first if blank
	@echo Press the backquote key in the page to open the overlay. Ctrl+C to stop.
	uv run --no-project python -m http.server $(HARNESS_PORT) --directory $(HARNESS_WEB)

# --- Agent bridge: drive + watch the DOOM game from outside the engine ---

# Headless: a JSON-over-HTTP server that drives the REAL DOOM game with no
# browser, so an external agent can send inputs and read back structured state.
#   curl -s -XPOST localhost:7878/step -d '{"keys":["forward"],"fire":true}'
agent:
	cargo run -p $(DOOM_CRATE) --features agent --bin agent

# Same, plus an offscreen wgpu render so `{"render":true}` returns a PNG path.
agent-render:
	cargo run -p $(DOOM_CRATE) --features agent-render --bin agent

# Bridge: relay HTTP actions to a LIVE browser opened with
# ?agent=ws://127.0.0.1:7879, and stream its frames back (canvas snapshots).
agent-bridge:
	cargo run -p $(DOOM_CRATE) --features agent --bin agent -- --bridge

# --- Mobile-first demo gallery (deployed by .github/workflows/deploy-pages.yml) ---

# Build EVERY browser demo's wasm bundle, build + vendor the @axiom/client SDK
# the netplay demo needs, and assemble the static gallery into dist/. Uses the
# same raw cargo + wasm-bindgen flow as the per-demo builds, then the portable
# Python assembler (scripts/assemble_gallery.py) so dist/ is identical locally
# and in CI. This is the build half of `make gallery`; CI's deploy-pages.yml runs
# the equivalent steps. Recipe stays portable (cargo/wasm-bindgen/npm/uv run all
# work under cmd.exe on Windows too).
gallery-build:
	cargo build -p $(BROWSER_CRATE) --target $(WASM_TARGET) --release
	cargo build -p $(NETPLAY_CRATE) --target $(WASM_TARGET) --release
	cargo build -p $(DOOM_CRATE) --target $(WASM_TARGET) --release
	cargo build -p $(STRESS_CRATE) --target $(WASM_TARGET) --release
	cargo build -p $(GROWTH_CRATE) --target $(WASM_TARGET) --release
	cargo build -p $(ROOMED_CRATE) --target $(WASM_TARGET) --release
	cargo build -p $(QUINTET_CRATE) --target $(WASM_TARGET) --release
	cargo build -p $(HARNESS_CRATE) --target $(WASM_TARGET) --release
	wasm-bindgen --target web --out-dir $(PKG_DIR) $(WASM_ARTIFACT)
	wasm-bindgen --target web --out-dir $(NETPLAY_PKG) $(NETPLAY_ARTIFACT)
	wasm-bindgen --target web --out-dir $(DOOM_PKG) $(DOOM_ARTIFACT)
	wasm-bindgen --target web --out-dir $(STRESS_PKG) $(STRESS_ARTIFACT)
	wasm-bindgen --target web --out-dir $(GROWTH_PKG) $(GROWTH_ARTIFACT)
	wasm-bindgen --target web --out-dir $(ROOMED_PKG) $(ROOMED_ARTIFACT)
	wasm-bindgen --target web --out-dir $(QUINTET_PKG) $(QUINTET_ARTIFACT)
	wasm-bindgen --target web --out-dir $(HARNESS_PKG) $(HARNESS_ARTIFACT)
	npm --prefix packages/axiom-client install --no-audit --no-fund
	npm --prefix packages/axiom-client run build
	uv run --no-project python -c "import shutil, pathlib; d = pathlib.Path('$(NETPLAY_WEB)/vendor/axiom-client'); shutil.rmtree(d, ignore_errors=True); d.parent.mkdir(parents=True, exist_ok=True); shutil.copytree('packages/axiom-client/dist', d)"
	uv run --no-project python scripts/assemble_gallery.py

# THE MAIN DRIVER. One command to browse the whole engine surface during
# development: it builds EVERY browser demo, assembles the static gallery into
# dist/, and serves it locally. It depends on gallery-build, so cargo's
# incremental compilation keeps re-runs fast after the first build. To re-serve
# WITHOUT rebuilding (e.g. after only restarting the server), use
# `make gallery-serve`.
gallery: gallery-build
	@echo Gallery built into $(DIST_DIR)/. Serving at http://localhost:$(GALLERY_PORT) - open in a WebGPU browser. Ctrl+C to stop.
	uv run --no-project python -m http.server $(GALLERY_PORT) --directory $(DIST_DIR)

# Serve the already-assembled gallery WITHOUT rebuilding (fast restart). Run
# `make gallery` (or `make gallery-build`) first if dist/ is missing or stale.
gallery-serve:
	@echo Serving prebuilt gallery at http://localhost:$(GALLERY_PORT) - run make gallery first if blank
	@echo Open it in a WebGPU browser. Ctrl+C to stop.
	uv run --no-project python -m http.server $(GALLERY_PORT) --directory $(DIST_DIR)

# --- Package a single game into a self-contained, droppable bundle ---

# Build ONE browser app into dist-app/<name>/: a wasm fast-path (wasm-opt -Oz) plus a
# Binaryen wasm2js fallback for browsers with no WebAssembly, behind a
# capability-detecting loader that prints one console.warn line when it falls back.
# (The engine's own WebGPU->WebGL2->Canvas2D backend fallback is orthogonal and lives
# in axiom-windowing; together they let even a no-wasm, no-WebGPU browser run a game.)
#
# APP is a short name (quintet) or an app dir (apps/axiom-quintet). Set INLINE=1 for a
# single self-contained index.html. The wasm2js fallback requires an MVP build, which
# needs a nightly toolchain with rust-src (-Z build-std); this target installs the
# pinned Binaryen toolchain on first run. The first build is slow (it rebuilds std).
#
#   make package APP=quintet
#   make package APP=quintet INLINE=1
APP ?= quintet
package:
	npm --prefix scripts/packaging install --no-audit --no-fund
	uv run --no-project python scripts/package_app.py $(APP) $(if $(INLINE),--inline,)

# --- TypeScript SDK gate (the @axiom/client static-analysis/branchless/coverage laws) ---

# Hold packages/axiom-client to TS-native versions of the engine's laws: tsgo
# (TypeScript 7.0 native) typecheck, Oxlint with every category an error plus the
# branch ban, and node:test 100% coverage. The TS counterpart of `bash
# scripts/coverage.sh`. Run `npm --prefix packages/axiom-client install` once
# first. The SDK is green and this gate is wired into pre-commit + CI as a hard gate.
ts-gate:
	bash scripts/ts-gate.sh
