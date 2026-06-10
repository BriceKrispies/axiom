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

.PHONY: demo demo-build help

help:
	@echo "Axiom tooling targets:"
	@echo "  make demo        Serve the browser rotating-cube slice at http://localhost:$(PORT) (uses uv)"
	@echo "  make demo-build  Rebuild the wasm bundle into web/pkg (needs wasm-pack)"
	@echo "  make PORT=9000 demo   Serve on a different port"

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
