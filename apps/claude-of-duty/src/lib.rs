//! **Claude of Duty** — the browser FPS at `C:/dev/Claude-of-Duty` (Three.js
//! r180, ISC licensed), ported onto Axiom.
//!
//! This crate is a composition leaf: every gameplay concept lives here and
//! nothing may depend on it. Engine *capability* the port needs — HDR render
//! targets, the frame-graph vocabulary, capsule contacts — lands in `crates/`
//! and `modules/` under the full weight of the Layer, Module, Branchless and
//! Coverage laws, and is consumed from here. The plan, and the placement
//! argument for each capability, is
//! `docs/work-manifests/claude-of-duty-port/00-manifest.md`.
//!
//! ## What is ported so far: the deterministic core
//!
//! `src/core/` of the source, minus `input.js` and `prewarm.js`. These four are
//! plain math and data with zero Three.js contact, and they are the substrate
//! everything else hangs off:
//!
//! | this crate        | source                    |
//! |-------------------|---------------------------|
//! | [`rng`]           | `src/core/rng.js`         |
//! | [`registry`]      | `src/core/registry.js` (the `Registry`) |
//! | [`events`]        | `src/core/registry.js` (the `EventBus`) |
//! | [`engine`]        | `src/core/engine.js`      |
//! | [`config`]        | `src/core/config.js`      |
//!
//! ## And the audio subsystem
//!
//! [`audio`] is `src/audio/` of the source — all 4,241 lines, module for
//! module. It is the most portable subsystem in the game and the highest value
//! per line: everything is synthesized, there is not one audio file anywhere,
//! and it has zero Three.js contact. The synthesis recipes and the DSP maths
//! port as ordinary Rust that computes sample buffers and parameter values,
//! testable natively with no browser; only [`audio::web_audio`] names a browser
//! API, and only on `wasm32`. See [`audio`] for the split and how it is pinned
//! against the JavaScript.
//!
//! Each module names its source file and line range, and comments every place
//! the Rust shape had to diverge from the JavaScript, with the reason at the
//! site. The port is meant to be diffable against the original by eye —
//! faithfulness before elegance, because the whole game's reproducibility is
//! downstream of these files behaving *exactly* as the source does.
//!
//! ## Determinism
//!
//! The source is fully seed-driven: one root seed, a disciplined [`rng::Rng::fork`]
//! per subsystem, and no other entropy anywhere. The port makes the root seed an
//! explicit argument to [`engine::Engine::new`], which is what makes a
//! frame-vs-frame comparison against the original meaningful at all.

pub mod audio;
pub mod config;
pub mod engine;
pub mod error;
pub mod events;
pub mod fx;
pub mod materials;
pub mod physics;
pub mod player;
pub mod registry;
pub mod rng;
pub mod sky;
pub mod ui;
pub mod viewer;
pub mod weapons;
pub mod world;
