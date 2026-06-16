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

# The live 2-browser lockstep multiplayer demo (apps/axiom-netplay-browser +
# the tools/axiom-netcode-relay server).
NETPLAY_DIR      := apps/axiom-netplay-browser
NETPLAY_CRATE    := axiom-netplay-browser
NETPLAY_ARTIFACT := target/$(WASM_TARGET)/release/axiom_netplay_browser.wasm
NETPLAY_WEB      := $(NETPLAY_DIR)/web
NETPLAY_PKG      := $(NETPLAY_WEB)/pkg
NETPLAY_PORT     ?= 8000

# The retro FPS-style first-person demo (apps/axiom-retro-fps-browser).
retro FPS_DIR         := apps/axiom-retro-fps-browser
retro FPS_CRATE       := axiom-retro-fps-browser
retro FPS_ARTIFACT    := target/$(WASM_TARGET)/release/axiom_retro_fps_browser.wasm
retro FPS_WEB         := $(retro FPS_DIR)/web
retro FPS_PKG         := $(retro FPS_WEB)/pkg
retro FPS_PORT        ?= 8000

# The N-spinning-cubes load/stress visual (apps/axiom-stress-cubes-browser).
STRESS_DIR       := apps/axiom-stress-cubes-browser
STRESS_CRATE     := axiom-stress-cubes-browser
STRESS_ARTIFACT  := target/$(WASM_TARGET)/release/axiom_stress_cubes_browser.wasm
STRESS_WEB       := $(STRESS_DIR)/web
STRESS_PKG       := $(STRESS_WEB)/pkg
STRESS_PORT      ?= 8000

GALLERY_DIR      := gallery
DIST_DIR         := dist
GALLERY_PORT     ?= 8000

.PHONY: demo demo-build netplay netplay-build relay retro_fps retro_fps-build stress stress-build gallery gallery-build help

help:
	@echo "Axiom tooling targets:"
	@echo "  make demo          Serve the browser rotating-cube slice at http://localhost:$(PORT) (uses uv)"
	@echo "  make demo-build    Rebuild the rotating-cube wasm bundle into web/pkg"
	@echo "  make PORT=9000 demo   Serve on a different port"
	@echo ""
	@echo "  Live 2-browser lockstep multiplayer demo:"
	@echo "  make relay         Run the WebSocket relay (ws://127.0.0.1:9001)"
	@echo "  make netplay-build Rebuild the netplay wasm bundle into its web/pkg"
	@echo "  make netplay       Serve the netplay page at http://localhost:$(NETPLAY_PORT)"
	@echo "  (run 'make relay' in one shell and 'make netplay' in another, then"
	@echo "   open the page in TWO WebGPU browsers and arrow-key your cube.)"
	@echo ""
	@echo "  retro FPS-style first-person demo:"
	@echo "  make retro_fps-build    Rebuild the retro_fps wasm bundle into its web/pkg"
	@echo "  make retro_fps          Serve the retro_fps page at http://localhost:$(retro FPS_PORT)"
	@echo ""
	@echo "  Load/stress visual (N spinning cubes):"
	@echo "  make stress-build  Rebuild the stress wasm bundle into its web/pkg"
	@echo "  make stress        Serve the stress page at http://localhost:$(STRESS_PORT)"
	@echo "  (open with ?cubes=N, or click the presets, to change the cube count.)"
	@echo ""
	@echo "  Mobile-first demo gallery (what deploy-pages.yml publishes):"
	@echo "  make gallery-build Build both wasm demos and assemble $(DIST_DIR)/"
	@echo "  make gallery       Serve $(DIST_DIR)/ at http://localhost:$(GALLERY_PORT)"

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

# --- Live 2-browser lockstep multiplayer demo ---

# The dumb broadcast relay: assigns each browser a peer id and forwards inputs.
# Run this first, in its own shell; leave it running.
relay:
	cargo run -p axiom-netcode-relay

# Rebuild the netplay wasm bundle (same raw cargo + wasm-bindgen flow as the
# rotating-cube demo, for a Cargo.lock-exact binding generator).
netplay-build:
	cargo build -p $(NETPLAY_CRATE) --target $(WASM_TARGET) --release
	wasm-bindgen --target web --out-dir $(NETPLAY_PKG) $(NETPLAY_ARTIFACT)

# Serve the netplay page. The relay (make relay) must already be running, then
# open this URL in TWO WebGPU browser windows. Recipe lines are kept portable
# (no sh-only test/||/{}) so make runs them under cmd.exe on Windows too; if the
# bundle is missing the page reports it, so run `make netplay-build` first.
netplay:
	@echo Serving netplay at http://localhost:$(NETPLAY_PORT) - run make netplay-build first if blank
	@echo Start the relay in another shell with:  make relay
	@echo Then open this URL in TWO WebGPU browser windows and arrow-key your cube.
	uv run --no-project python -m http.server $(NETPLAY_PORT) --directory $(NETPLAY_WEB)

# --- retro FPS-style first-person demo ---

# Rebuild the retro_fps wasm bundle (same raw cargo + wasm-bindgen flow).
retro_fps-build:
	cargo build -p $(retro FPS_CRATE) --target $(WASM_TARGET) --release
	wasm-bindgen --target web --out-dir $(retro FPS_PKG) $(retro FPS_ARTIFACT)

# Serve the retro_fps page. Run `make retro_fps-build` first if blank. Open in a WebGPU
# browser; tank controls (arrows/WASD + Space).
retro_fps:
	@echo Serving retro_fps at http://localhost:$(retro FPS_PORT) - run make retro_fps-build first if blank
	@echo Open it in a WebGPU browser. Tank controls: arrows/WASD to move+turn, Space to fire.
	uv run --no-project python -m http.server $(retro FPS_PORT) --directory $(retro FPS_WEB)

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

# --- Mobile-first demo gallery (deployed by .github/workflows/deploy-pages.yml) ---

# Build both wasm demos and assemble the static gallery bundle into dist/. Uses
# the same raw cargo + wasm-bindgen flow as the per-demo builds, then a portable
# Python assembler (scripts/assemble_gallery.py) so dist/ is identical locally
# and in CI. Recipe stays portable (cargo/wasm-bindgen/uv run all work under
# cmd.exe on Windows too).
gallery-build:
	cargo build -p $(BROWSER_CRATE) --target $(WASM_TARGET) --release
	cargo build -p $(NETPLAY_CRATE) --target $(WASM_TARGET) --release
	cargo build -p $(retro FPS_CRATE) --target $(WASM_TARGET) --release
	cargo build -p $(STRESS_CRATE) --target $(WASM_TARGET) --release
	wasm-bindgen --target web --out-dir $(PKG_DIR) $(WASM_ARTIFACT)
	wasm-bindgen --target web --out-dir $(NETPLAY_PKG) $(NETPLAY_ARTIFACT)
	wasm-bindgen --target web --out-dir $(retro FPS_PKG) $(retro FPS_ARTIFACT)
	wasm-bindgen --target web --out-dir $(STRESS_PKG) $(STRESS_ARTIFACT)
	uv run --no-project python scripts/assemble_gallery.py

# Serve the assembled gallery. Run `make gallery-build` first if dist/ is blank.
gallery:
	@echo Serving demo gallery at http://localhost:$(GALLERY_PORT) - run make gallery-build first if blank
	@echo Open it in a WebGPU browser. Ctrl+C to stop.
	uv run --no-project python -m http.server $(GALLERY_PORT) --directory $(DIST_DIR)
