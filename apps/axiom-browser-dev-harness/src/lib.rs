//! # Axiom — Browser Debug Overlay developer harness
//!
//! A thin host for the [`axiom_debug_overlay`] module: it mounts the overlay +
//! command console over a bare canvas and feeds it **real** browser-side
//! diagnostics. The overlay's whole state machine (density, command registry,
//! console history, keyboard classification) — and its DOM binding — live in the
//! module, behind the `DebugOverlayApi` facade; this app only measures values and
//! pushes them in.
//!
//! ## Where the diagnostics come from (real, not stub)
//! Every value the harness feeds is measured, probed, or an honest constant —
//! there is no fabricated stub provider:
//! - **fps / frame time** — measured from `requestAnimationFrame` deltas.
//! - **frame index** — the RAF frame counter.
//! - **visibility** — `document`'s hidden flag.
//! - **renderer backend / fallback** — a real `navigator.gpu` capability probe,
//!   reporting the engine's actual WebGPU→WebGL2 fallback choice.
//! - engine-internal fields the harness genuinely cannot observe without running
//!   the engine (sim ticks, GPU submissions, worker messages) are honest zeroes;
//!   absent subsystems (storage/audio/network) are honest `none`.
//!
//! All of this is the nondeterministic browser edge, so it is confined to the
//! `wasm32` [`web`] module; native `cargo test` compiles nothing here.

#[cfg(target_arch = "wasm32")]
mod web;
