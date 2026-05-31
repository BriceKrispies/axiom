//! # Axiom — Browser/WASM Rotating-Cube App (visible slice)
//!
//! The browser-visible counterpart to the headless deterministic slice
//! (`apps/axiom-demo-rotating-cube`). This is the **only** crate in the
//! workspace allowed to use browser/WASM platform APIs (`wasm-bindgen`,
//! `web-sys`, `js-sys`) and a real `wgpu` binding — and even here they are
//! confined to `#[cfg(target_arch = "wasm32")]` code so native builds and
//! `cargo test --workspace` never pull them in.
//!
//! ```text
//! BrowserRotatingCubeApi (wasm)
//!   → find <canvas id="axiom-cube-canvas">
//!   → build HostPresentationRequest (deterministic, browser-free)
//!   → WebGpuApi::new_live(request)
//!   → initialise real wgpu surface/adapter/device/queue  (wasm32)
//!   → requestAnimationFrame loop:
//!       → reuse the SAME rotating-cube GpuSubmission as recording mode
//!       → clear + present the canvas through the live binding
//! ```
//!
//! See `README.md` and `VISIBLE_SLICE.md` for what renders today and what
//! remains blocked.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`BrowserRotatingCubeApi`]. The
//! deterministic driver, surface registry, live-binding state machine, and
//! tick outcome are internal and reached only through it.

mod browser_api;
mod browser_bootstrap;
mod browser_surface_registry;
mod cube_slice;
mod live_gpu_binding;
mod render_loop;

pub use browser_api::BrowserRotatingCubeApi;
