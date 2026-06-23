//! # Axiom — Browser Debug Overlay & Command Console (developer harness)
//!
//! A developer debug overlay for the live browser/WASM engine surface, plus a
//! tiny in-overlay command console, mounted over a bare canvas by this harness
//! app. Toggle it with the physical **Backquote** (`` ` ``) key.
//!
//! ## Architectural placement — this is *app-side developer tooling*
//!
//! The deterministic engine spine (kernel / runtime / math / the layers and
//! modules) must never learn about the DOM, keyboard events, CSS, canvas,
//! WebGPU, browser timing, or command text. So none of that lives in a layer or
//! a module — it lives here, in an **app**, the only tier permitted to reference
//! `web_sys` outside the platform-facing `host` layer and `windowing` module
//! (Module Law #9). Apps are also outside the Branchless and Coverage laws,
//! which is why this code reads as ordinary idiomatic Rust.
//!
//! ## The split: pure logic (native-tested) vs. the DOM edge (wasm-only)
//!
//! Everything that *decides* — density cycling, keyboard-shortcut
//! classification, the command registry/dispatcher, the diagnostics snapshot —
//! is pure, deterministic, browser-free Rust that compiles on **native** and is
//! covered by ordinary `#[test]`s run under `cargo test --workspace`. The DOM
//! controller ([`debug_overlay::DebugOverlayController`]) and the
//! `#[wasm_bindgen]` entry are the thin nondeterministic edge, compiled only for
//! `wasm32` and verified in a real browser (see `DEBUG_OVERLAY.md`).
//!
//! ```text
//! browser keydown ─▶ browser_keyboard_shortcut::classify ─▶ OverlayShortcut
//!                                                              │
//!   console submit ─▶ CommandRegistry::execute ─▶ CommandResult│
//!                                                              ▼
//!                                                        OverlayState  (pure)
//!                                                              │
//!                                       DebugOverlayController.sync (DOM, wasm)
//! ```
//!
//! ## Diagnostics are host-fed, never engine state
//!
//! The overlay only ever *reads* a [`browser_diagnostics::BrowserDiagnosticsSnapshot`]
//! handed to it by the host. The values here come from a replaceable
//! [`browser_diagnostics::StubDiagnosticsProvider`]; a real host swaps in its own
//! provider. The overlay must never become a source of deterministic engine
//! state — it is a read-out, not a model.

// The pure, browser-free overlay logic. `pub` so it is the crate's tested public
// API on every target (and so it is never dead code on a native build).
pub mod browser_diagnostics;
pub mod browser_keyboard_shortcut;
pub mod debug_command;
pub mod debug_command_registry;
pub mod debug_console;
pub mod debug_overlay_density;
pub mod debug_overlay_state;

// The DOM edge: the controller that projects pure overlay state onto real DOM
// nodes, and the `#[wasm_bindgen]` browser entry that mounts it. Compiled only
// for the browser; never seen by native `cargo test`.
#[cfg(target_arch = "wasm32")]
pub mod debug_overlay;
#[cfg(target_arch = "wasm32")]
mod web;
