# Axiom — repo tooling Makefile.
#
# This is repo tooling (alongside scripts/), NOT part of the engine
# dependency graph. It declares no package and is invisible to the Layer,
# Module, and App laws — same status as the xtask crate and the coverage
# scripts.
#
# Primary target: `make gallery` packages every REGISTERED app into dist/<id>/
# behind the static landing grid (apps/axiom-gallery/web), and serves the packaged
# dist/ over http://localhost. WebGPU requires an http:// origin, so a plain
# file:// open will not work.
#
# Apps register THEMSELVES: an app joins the gallery by carrying an app.json in its
# own directory, and scripts/package_gallery.py discovers them. There are no
# per-app targets in this file — there used to be, and five of the seven had rotted
# into pointing at apps that no longer existed. To add an app, add its app.json
# (`cargo run -p axiom-serve -- init <app>` writes one); to remove it, delete the
# app. Nothing here needs touching either way.

WASM_TARGET      := wasm32-unknown-unknown
# The gallery's static landing grid (card grid + shared styles) — a plain web
# dir, no crate. It holds no per-app pages: each app's page is built from source.
GALLERY_DIR      := apps/axiom-gallery
GALLERY_WEB      := $(GALLERY_DIR)/web
DIST_DIR         := dist
GALLERY_PORT     ?= 8000
WORKSPACE_PORT   ?= 8123

# The shared @axiom/game runtime wasm, which hosts the SDK-hosted TypeScript apps
# (a separate, app-tier mechanism from the pure-TS @axiom/web-engine path the
# gallery's TypeScript apps use).
GAME_RUNTIME_CRATE    := axiom-game-runtime
GAME_RUNTIME_PKG      := apps/axiom-game-runtime/web/pkg
GAME_RUNTIME_ARTIFACT := target/$(WASM_TARGET)/release/axiom_game_runtime.wasm

# End Zone: the arcade-football engine framework + deterministic showcase
# (its own standalone app — not part of the gallery).
ENDZONE_DIR      := apps/end-zone
ENDZONE_CRATE    := axiom-end-zone
ENDZONE_ARTIFACT := target/$(WASM_TARGET)/release/axiom_end_zone.wasm
ENDZONE_WEB      := $(ENDZONE_DIR)/web
ENDZONE_PKG      := $(ENDZONE_WEB)/pkg
ENDZONE_PORT     ?= 8000

.PHONY: workspace workspace-build \
	gallery gallery-build gallery-serve gallery-fast gallery-fast-build \
	gallery-debug-build render-bench \
	netplay-server relay retro-fps-hot \
	agent agent-render agent-bridge \
	end-zone end-zone-build \
	package loader-test e2e e2e-ladder \
	hostile-harness \
	netplay-load serve ts-gate help \
	sound sound-check sound-build sound-list sound-clean sound-test

help:
	@echo "Axiom tooling targets:"
	@echo ""
	@echo "  ===> MAIN DRIVER — the app gallery (every REGISTERED app PACKAGED into dist/ + served):"
	@echo "  make gallery        PACKAGE every registered app, assemble dist/, serve at http://localhost:$(GALLERY_PORT)"
	@echo "  make gallery-fast   Quick wasm-only gallery (no fallback, normal incremental build) — seconds, for iteration"
	@echo "  make gallery-serve  Re-serve the already-built dist/ WITHOUT rebuilding (fast restart)"
	@echo "  make gallery-build  Package the app bundles + assemble dist/ only, no serve"
	@echo "  make GALLERY_PORT=9000 gallery   Serve on a different port"
	@echo "  (make gallery is slow the first time — it rebuilds std MVP so the wasm2js fallback is possible.)"
	@echo ""
	@echo "  Apps REGISTER THEMSELVES — an app is in the gallery iff it has an app.json:"
	@echo "  cargo run -p axiom-serve -- init <app>          Write that app's app.json (detects its kind)"
	@echo "  uv run --no-project python scripts/package_gallery.py --list   List every registered app"
	@echo "  (TypeScript apps share ONE @axiom/web-engine build at dist/engine/web-engine/<version>/;"
	@echo "   Rust apps statically link the engine into their own wasm, as wasm requires.)"
	@echo ""
	@echo "  ===> DEV CONSOLE — the axiom-workspace (loads every gallery app + games/ cartridges):"
	@echo "  make workspace      Build the console (shell + gallery bundle) + serve at http://localhost:$(WORKSPACE_PORT)"
	@echo "  make workspace-build  Build dist-workspace/ only, no serve"
	@echo "  (hosts every app inline or opens the multi-screen ones; has the frame scrubber + backend-compare dev tools.)"
	@echo ""
	@echo "  Server-authoritative multiplayer:"
	@echo "  make netplay-server Run the Rust authoritative server (ws://127.0.0.1:9002)"
	@echo "  make netplay-load   Load-test a running node/cluster (ARGS=\"<soak|matchmake|scaleout|resilience> ...\")"
	@echo ""
	@echo "  retro FPS live level hot-reload:"
	@echo "  make retro-fps-hot       Build the fast gallery + serve retro FPS with live level hot-reload at http://localhost:8080/retro-fps/"
	@echo "  (edit apps/axiom-retro-fps/src/level.axiom and save to reload the level live.)"
	@echo ""
	@echo "  Agent drivers (native, feature-gated bins of the demo app crates):"
	@echo "  make agent          retro FPS headless agent server (JSON over HTTP on :7878)"
	@echo "  make agent-render   Same, plus an offscreen wgpu render so {\"render\":true} returns a PNG"
	@echo "  make agent-bridge   Relay HTTP actions to a LIVE browser opened with ?agent=ws://127.0.0.1:7879"
	@echo ""
	@echo ""
	@echo "  End Zone arcade-football showcase (standalone, not in the gallery):"
	@echo "  make end-zone-build     Rebuild the End Zone wasm bundle into web/pkg"
	@echo "  make end-zone           Serve End Zone at http://localhost:$(ENDZONE_PORT)"
	@echo ""
	@echo "  Package ONE single-page app into a self-contained, droppable bundle (wasm + wasm2js fallback):"
	@echo "  make package APP=game-runtime      Build dist-app/game-runtime/ (an SDK-hosted TypeScript app)"
	@echo "  make package APP=burnt-rubber      Build a native single-page app"
	@echo "  (the whole MULTI-PAGE gallery is packaged by 'make gallery-build' into dist/, not 'make package'.)"
	@echo "  (needs a nightly toolchain with rust-src; first build rebuilds std and is slow.)"
	@echo "  make loader-test   Prove the loader's wasm→wasm2js fallback (Node-only, seconds)"
	@echo ""
	@echo "  Browser end-to-end smoke tests (pytest-playwright):"
	@echo "  make e2e           Build+serve the gallery and drive every non-multiplayer demo in a real browser"
	@echo "  AXIOM_E2E_REUSE=1 make e2e   Reuse a gallery already serving on :8000 (skip the rebuild)"
	@echo "  make e2e-ladder    Deny capabilities to the resilient chest game and prove every rung still pays out"
	@echo "  make hostile-harness  Serve the developer harness for the same ladder at http://localhost:8091/__harness/"
	@echo ""
	@echo "  TypeScript SDK gate (@axiom/client + @axiom/game static-analysis/branchless/coverage laws):"
	@echo "  make ts-gate       Run tsgo typecheck + Oxlint + co-location + 100% coverage for both TS packages"

# --- Mobile-first demo gallery (deployed by .github/workflows/deploy-pages.yml) ---

# PACKAGE the demo gallery into dist/ via scripts/package_gallery.py: every demo app's
# own wasm bundle (the shipping tuning's wasm-opt -O3 fast-path PLUS a Binaryen wasm2js
# fallback for browsers with no WebAssembly) into dist/<id>/ behind its
# capability-detecting loader, with the static landing grid laid over it. First it
# installs the pinned Binaryen toolchain and builds the @axiom/client SDK.
#
# This is the build half of `make gallery`. Because the app is rebuilt MVP via nightly
# `-Z build-std` (so the wasm2js fallback is possible), the FIRST run is slow — it
# compiles std MVP once into the shared target/package-mvp dir; re-runs are incremental.
# Needs a nightly toolchain with rust-src. (`make gallery-fast` keeps the quick
# wasm-only flow with no fallback for tight iteration.)
gallery-build:
	npm --prefix scripts/packaging install --no-audit --no-fund
	npm --prefix packages/axiom-client install --no-audit --no-fund
	npm --prefix packages/axiom-client run build
	uv run --no-project python scripts/package_gallery.py

# THE MAIN DRIVER. One command to browse the whole engine surface during
# development: it builds every demo app bundle, assembles the static gallery into
# dist/, and serves it locally. It depends on gallery-build, so cargo's incremental
# compilation keeps re-runs fast after the first build. To re-serve WITHOUT
# rebuilding, use `make gallery-serve`.
gallery: gallery-build
	@echo Gallery built into $(DIST_DIR)/. Serving at http://localhost:$(GALLERY_PORT) - open in a WebGPU browser. Ctrl+C to stop.
	uv run --no-project python -m http.server $(GALLERY_PORT) --directory $(DIST_DIR)

# Serve the already-assembled gallery WITHOUT rebuilding (fast restart). Run
# `make gallery` (or `make gallery-build`) first if dist/ is missing or stale.
gallery-serve:
	@echo Serving prebuilt gallery at http://localhost:$(GALLERY_PORT) - run make gallery first if blank
	@echo Open it in a WebGPU browser. Ctrl+C to stop.
	uv run --no-project python -m http.server $(GALLERY_PORT) --directory $(DIST_DIR)

# Fast iteration variant: packages the gallery wasm-only (a normal incremental release
# build through the same loader, NO MVP/build-std rebuild and NO wasm2js fallback), then
# serves dist/. Seconds, not minutes — use this while iterating; use `make gallery` for
# the deploy-grade bundle with the fallback.
# `--tuning preview` is what makes this the "seconds" target: thin LTO + wasm-opt -Oz.
# The bundles it produces are for LOOKING at, not for measuring frame rate on — the
# deployed/shipping tuning is `make gallery` (and the Pages workflow).
gallery-fast-build:
	npm --prefix scripts/packaging install --no-audit --no-fund
	uv run --no-project python scripts/package_gallery.py --fast --tuning preview

gallery-fast: gallery-fast-build
	@echo Fast gallery (wasm-only) built into $(DIST_DIR)/. Serving at http://localhost:$(GALLERY_PORT) - Ctrl+C to stop.
	uv run --no-project python -m http.server $(GALLERY_PORT) --directory $(DIST_DIR)

# --- Workspace dev console (loads every gallery app + the games/ cartridges) ---

# Build + serve the axiom-workspace dev console: compiles the vanilla-TS shell with
# tsgo, lays it into dist-workspace/, and builds the ONE gallery bundle into
# dist-workspace/gallery/ so the console can load every gallery app (inline single-
# canvas boot, or open the multi-screen ones) plus the retro_fps cartridge, and run the
# no-iframe backend-compare tool. Fast wasm-only bundle (seconds after the first
# cargo build); the shell's own extension-resolving static server serves it.
workspace:
	uv run --no-project python scripts/package_workspace.py --serve --port $(WORKSPACE_PORT)

# Build only (no serve): assemble dist-workspace/.
workspace-build:
	uv run --no-project python scripts/package_workspace.py

# A debug wasm gallery build: keeps debug_assertions on, so the Canvas2D deep
# profiler (the convert project/shade split) is present. Used by `make render-bench`.
gallery-debug-build:
	npm --prefix scripts/packaging install --no-audit --no-fund
	uv run --no-project python scripts/package_gallery.py --debug

# RENDER BENCHMARK: build+serve the gallery, auto-walk a demo (default burnt-rubber) with
# the agent, and report FPS + phase breakdown from the Canvas2D telemetry. Pass extra
# flags via ARGS, e.g. `make render-bench ARGS="--backend canvas2d --duration 10 --debug"`.
render-bench:
	cargo run -q -p axiom-render-bench -- $(ARGS)

# --- Live 2-browser SERVER-AUTHORITATIVE multiplayer demo ---

# The authoritative game server: holds the state, accepts JoinRoom/ClientIntent,
# and broadcasts ServerSnapshots over the axiom-net-protocol wire format. Run
# this first, in its own shell; leave it running.
netplay-server:
	cargo run -p axiom-netplay-server


# The dumb lockstep broadcast relay (legacy tooling; the netplay demo no longer
# uses it, but the tool is kept for lockstep experiments).
relay:
	cargo run -p axiom-netcode-relay


# --- retro FPS live level hot-reload ---

# Serve retro FPS with LIVE LEVEL HOT-RELOAD. Builds the fast gallery into dist/ first
# (so the retro FPS bundle + page exist at dist/retro-fps/), then the axiom-dev-reload
# dev server serves dist/ and additionally watches level.axiom, pushing every saved edit
# to the browser over SSE — edit a wall and watch it update with no recompile and no
# reload. Open http://localhost:8080/retro-fps/ and edit apps/axiom-retro-fps/src/level.axiom.
retro-fps-hot: gallery-fast-build
	@echo Serving retro FPS with hot-reload at http://localhost:8080/retro-fps/ - edit apps/axiom-retro-fps/src/level.axiom and save.
	cargo run -p axiom-dev-reload

# --- Agent bridge: drive + watch the retro FPS game from outside the engine ---

# Headless: a JSON-over-HTTP server that drives the REAL retro FPS game with no
# browser, so an external agent can send inputs and read back structured state.
#   curl -s -XPOST localhost:7878/step -d '{"keys":["forward"],"fire":true}'
agent:
	cargo run -p axiom-retro-fps --features agent --bin retro-fps-agent

# Same, plus an offscreen wgpu render so `{"render":true}` returns a PNG path.
agent-render:
	cargo run -p axiom-retro-fps --features agent-render --bin retro-fps-agent

# Bridge: relay HTTP actions to a LIVE browser opened with
# ?agent=ws://127.0.0.1:7879, and stream its frames back (canvas snapshots).
agent-bridge:
	cargo run -p axiom-retro-fps --features agent --bin retro-fps-agent -- --bridge



# --- Build + serve any apps/ browser app locally with hot reload ---

# tools/axiom-serve: resolve APP (short name, axiom- name, or path), detect its
# shape (Rust wasm via wasm-bindgen, or TypeScript over @axiom/game /
# @axiom/web-engine / plain tsgo), build it, serve its web/ with the vendor/pkg
# routes and SSE hot reload, and rebuild + reload the browser on save. Extra
# flags via ARGS, e.g. `make serve APP=home-run ARGS="--port 9000 --no-open"`.
serve:
	cargo run -p axiom-serve -- $(APP) $(ARGS)

# --- tools/axiom-sound: Strudel game-sound asset pipeline ---
# A Tool (npm package, off the engine graph and the coverage/branchless gates).
# Authors, checks, renders, and builds Strudel sound sources into an app's
# assets/audio/. Select the target app with APP=<app-path>, e.g.
# `make sound-build APP=apps/my-app`; extra flags via ARGS (e.g. --name id).
SOUND_DIR := tools/axiom-sound

sound:
	npm --prefix $(SOUND_DIR) install --no-audit --no-fund

sound-check:
	npm --prefix $(SOUND_DIR) run check -- --app $(APP) $(ARGS)

sound-build:
	npm --prefix $(SOUND_DIR) run build -- --app $(APP) $(ARGS)

sound-list:
	npm --prefix $(SOUND_DIR) run list -- --app $(APP) $(ARGS)

sound-clean:
	npm --prefix $(SOUND_DIR) run clean -- --app $(APP) $(ARGS)

sound-test:
	npm --prefix $(SOUND_DIR) test

# --- End Zone (apps/end-zone) ---

# Rebuild the End Zone wasm bundle (raw cargo + wasm-bindgen flow).
# (`make serve APP=end-zone` is the hot-reload alternative.)
end-zone-build:
	cargo build -p $(ENDZONE_CRATE) --target $(WASM_TARGET) --release
	wasm-bindgen --target web --out-dir $(ENDZONE_PKG) $(ENDZONE_ARTIFACT)
	@# Keep the served menu-music copy in sync with a fresh axiom-sound render
	@# (its staging dir is git-ignored; web/audio/menu.mp3 is the shipped asset).
	@if [ -f $(ENDZONE_DIR)/assets/audio/menu.mp3 ]; then \
		mkdir -p $(ENDZONE_WEB)/audio && \
		cp $(ENDZONE_DIR)/assets/audio/menu.mp3 $(ENDZONE_WEB)/audio/menu.mp3; \
	fi

# Serve the End Zone showcase. Run `make end-zone-build` first.
end-zone:
	@echo Serving End Zone at http://localhost:$(ENDZONE_PORT) - run make end-zone-build first
	uv run --no-project python -m http.server $(ENDZONE_PORT) --directory $(ENDZONE_WEB)

# --- Package a single app into a self-contained, droppable bundle ---

# Build ONE browser app into dist-app/<name>/: a wasm fast-path (wasm-opt -Oz) plus a
# Binaryen wasm2js fallback for browsers with no WebAssembly, behind a
# capability-detecting loader that prints one console.warn line when it falls back.
# (The engine's own WebGPU->WebGL2->Canvas2D backend fallback is orthogonal and lives
# in axiom-windowing; together they let even a no-wasm, no-WebGPU browser run a game.)
#
# APP is a short name (game-runtime) or an app dir (apps/axiom-game-runtime). Set
# INLINE=1 for a single self-contained index.html. This packager is for SINGLE-PAGE
# apps; the multi-page gallery is packaged by `make gallery-build` (it lays a static
# site over one shared bundle), not here. The wasm2js fallback requires an MVP build,
# which needs a nightly toolchain with rust-src (-Z build-std); this target installs
# the pinned Binaryen toolchain on first run. The first build is slow (it rebuilds std).
#
# SDK-hosted TypeScript apps (game-runtime, authored over @axiom/game) package too.
#
#   make package APP=game-runtime
#   make package APP=burnt-rubber
APP ?= game-runtime
package:
	npm --prefix scripts/packaging install --no-audit --no-fund
	uv run --no-project python scripts/package_app.py $(APP) $(if $(INLINE),--inline,)

# Prove the packaged loader's wasm→wasm2js fallback decision (scripts/package_app.py
# loader templates): instantiates the generated loader JS in Node with WebAssembly
# forced absent / rejecting / working, and asserts the fallback fires on EITHER an
# absent API OR an instantiation failure. Node-only, no browser, no nightly build —
# seconds. Also runs as part of `make e2e`.
loader-test:
	uv run --no-project --with pytest pytest e2e/test_loader_fallback.py -q

# --- Browser end-to-end smoke tests (pytest-playwright) ---

# Drive the gallery in a real browser: enter every non-multiplayer demo (default +
# ?backend=canvas2d), assert it loaded (ready signal, no FATAL console error) and the
# canvas actually painted. conftest.py builds the fast gallery + serves dist/ on :8000
# for the session. uv resolves the test deps ephemerally; the first run also downloads
# Chromium. Set AXIOM_E2E_REUSE=1 to reuse a gallery already serving on :8000.
E2E_UV := uv run --no-project --with pytest --with pytest-playwright --with pillow
e2e:
	$(E2E_UV) python -m playwright install chromium
	$(E2E_UV) pytest e2e -q --ignore=e2e/test_capability_ladder.py

# Walk the whole capability ladder (webgpu -> webgl2 -> webgl1 -> canvas2d ->
# css3d -> form) against the REAL resilient chest game, and assert at every rung
# both that the expected rung was selected AND that a pick still reached
# POST /api/pick and came back readable. Render rungs are denied in-realm by
# tools/axiom-harness/web/caps-mask.js (no sandbox token, CSP directive or
# Permissions-Policy feature can remove a rendering API — measured); webgl1 and
# canvas2d are ALSO denied by real Chromium launch flags as a second opinion; the
# no-JS rungs use java_script_enabled=False and a CSP script-src 'none'. The test
# module starts its own server (tools/axiom-harness, which embeds the real
# axiom-chest-server) and builds the engine dist, so nothing needs to be running
# first. AXIOM_LADDER_REUSE=1 reuses one already on :8091.
e2e-ladder:
	$(E2E_UV) python -m playwright install chromium
	$(E2E_UV) pytest e2e/test_capability_ladder.py -q

# The same ladder, by hand: a developer harness that hosts the real game in an
# iframe and takes capabilities away from it with radio buttons + checkboxes, then
# reads back which rung it landed on. It states plainly which toggles it CANNOT
# enforce (the launch-flag ones — a page cannot relaunch its own browser) and
# prints their command lines instead.
hostile-harness:
	node tools/axiom-harness/src/main.ts --port 8091


# Headless load generator (tools/axiom-netplay-load): opens many concurrent
# WebSocket players speaking the real wire protocol to stress a running node or
# cluster. Start a server first (e.g. `make netplay-server`), set AXIOM_LAG_MS=16
# to disable the demo's snapshot lag,
# then point the tool at it. `make netplay-load` runs a default single-node soak;
# override the scenario/flags with ARGS, e.g.:
#   make netplay-load ARGS="matchmake --requests 500"
#   make netplay-load ARGS="scaleout --target http://localhost:8100 --players 40"
#   make netplay-load ARGS="resilience --players 4 --rooms 2 --kill-every 3"
NETPLAY_LOAD_ARGS ?= soak --players 100 --rooms 50 --duration 10 --min-tick-advance 200
netplay-load:
	cargo run -q -p axiom-netplay-load -- $(if $(ARGS),$(ARGS),$(NETPLAY_LOAD_ARGS))

# --- TypeScript SDK gate (the @axiom/client static-analysis/branchless/coverage laws) ---

# Hold packages/axiom-client to TS-native versions of the engine's laws: tsgo
# (TypeScript 7.0 native) typecheck, Oxlint with every category an error plus the
# branch ban, and node:test 100% coverage. The TS counterpart of `bash
# scripts/coverage.sh`. Run `npm --prefix packages/axiom-client install` once
# first. The SDK is green and this gate is wired into pre-commit + CI as a hard gate.
ts-gate:
	bash scripts/ts-gate.sh

# ---------------------------------------------------------------------------
# ax - the query-and-change gateway for this repo (tools/axiom-atlas).
#
# Installs `ax` into ~/.cargo/bin so it can be called from any directory instead
# of building and invoking target/release/ax.exe. The tool locates its repo by
# walking up from the cwd, keeps its index and ledger per-repo under
# .axiom-atlas/, and re-execs its own binary for the background observer, so an
# installed copy behaves identically to a local build.
#
# Reinstall after changing the tool; an installed copy does not track the repo.
.PHONY: install-ax
install-ax:
	cargo install --path tools/axiom-atlas --profile ax --force
