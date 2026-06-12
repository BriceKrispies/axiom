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

.PHONY: demo demo-build netplay netplay-build relay help

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

# Serve the prebuilt wasm bundle. uv provides/manages the Python interpreter;
# --no-project keeps it from trying to sync a Python project in the repo root.
demo:
	@test -f "$(PKG_DIR)/axiom_demo_rotating_cube_browser_bg.wasm" \
		|| { echo "No wasm bundle in $(PKG_DIR). Run 'make demo-build' first."; exit 1; }
	@echo "Serving $(WEB_DIR) at http://localhost:$(PORT)"
	@echo "Open it in a WebGPU browser (recent Chrome/Edge, or Firefox Nightly). Ctrl+C to stop."
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
# open this URL in TWO WebGPU browser windows.
netplay:
	@test -f "$(NETPLAY_PKG)/axiom_netplay_browser_bg.wasm" \
		|| { echo "No wasm bundle in $(NETPLAY_PKG). Run 'make netplay-build' first."; exit 1; }
	@echo "Serving $(NETPLAY_WEB) at http://localhost:$(NETPLAY_PORT)"
	@echo "Make sure 'make relay' is running, then open this URL in TWO WebGPU"
	@echo "browser windows. Arrow-key your cube; the other window sees it move."
	uv run --no-project python -m http.server $(NETPLAY_PORT) --directory $(NETPLAY_WEB)
