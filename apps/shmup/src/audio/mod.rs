//! **Audio** — synthesized weapon/foley audio, spatialisation, reverb,
//! occlusion, mix.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/audio/` — all 4,241 lines of it,
//! module for module:
//!
//! | this module        | source            | lines |
//! |--------------------|-------------------|-------|
//! | [`dsp`]            | `dsp.js`          | 330   |
//! | [`ir`]             | `ir.js`           | 250   |
//! | [`weapons`]        | `weapons.js`      | 362   |
//! | [`foley`]          | `foley.js`        | 789   |
//! | [`vox`]            | `vox.js`          | 322   |
//! | [`mixer`]          | `mixer.js`        | 347   |
//! | [`spatial`]        | `spatial.js`      | 347   |
//! | [`ambience`]       | `ambience.js`     | 369   |
//! | [`system`]         | `index.js`        | 868   |
//! | [`graph`]          | the Web Audio surface those files are written against |
//! | [`web_audio`]      | the browser edge, `wasm32` only |
//!
//! There is not a single audio file in the project. Every gunshot, footstep,
//! shell casing, enemy shout and reverb tail is synthesized at runtime from
//! numbers in these files.
//!
//! ## How the port is split, and why
//!
//! The source is written against `BaseAudioContext` rather than the live
//! `AudioContext` for one specific reason, which its own `selftest.js` spells
//! out: it makes the entire synthesis path renderable in an
//! `OfflineAudioContext`, with no user gesture and no speaker, so the subsystem
//! can be verified headlessly. That property is the hinge the whole port turns
//! on, and it is preserved and strengthened here:
//!
//! * **The recipes are the content.** Which nodes, at which frequencies, with
//!   which envelopes, in which order, driven by which `rng` draws — that is what
//!   4,241 lines of `audio/` actually *are*, and none of it is a browser
//!   concern. It is ported as ordinary Rust that writes into an
//!   [`graph::AudioGraph`]: a recording stand-in for `BaseAudioContext` with the
//!   same vocabulary and the same defaults.
//! * **The maths is dependency-free.** [`ir::generate_ir`] takes a sample rate,
//!   a spec and an [`Rng`](crate::rng::Rng) and returns sample buffers.
//!   [`dsp::fill_noise`], [`dsp::saturation_curve`], [`dsp::limiter_curve`] and
//!   [`ir::classify_space`] are the same shape. A convolver only appears at the
//!   very edge, when [`mixer::Mixer::build_reverbs`] hands a rendered buffer to
//!   one.
//! * **The browser is one file.** [`web_audio`] walks a recorded graph and
//!   instantiates it. It is `wasm32`-only, so native `cargo test` never compiles
//!   it, and it has no decisions of its own to get wrong.
//!
//! ## How it is verified
//!
//! `tests/audio_port.rs` compares the port against **goldens captured from the
//! original JavaScript running under Node**, not against recomputations:
//!
//! * every sample of a rendered impulse response, and the head, tail and
//!   checksums of every noise colour — exactly, on the `f32` values a
//!   `Float32Array` store actually rounds to;
//! * the saturation and limiter curve tables at fixed indices;
//! * every automation event the envelope helpers emit, including the guard arms
//!   that refuse a NaN;
//! * [`ir::classify_space`] over seven room shapes, field for field;
//! * and, for two dozen voices, **the entire graph** — node list in creation
//!   order with every constructed parameter, every connection, every automation
//!   event and every source start — against the same voice built by the same
//!   seed in JavaScript. That is what makes this a port rather than a
//!   re-implementation: a drift of one `rng` draw anywhere moves hundreds of
//!   numbers and the test says exactly which.
//!
//! Transcendental-derived values are compared within a stated relative
//! tolerance; everything reachable by exact arithmetic is compared exactly.

pub mod ambience;
pub mod dsp;
pub mod foley;
pub mod graph;
pub mod ir;
pub mod mixer;
pub mod spatial;
pub mod system;
pub mod vox;
pub mod weapons;

#[cfg(target_arch = "wasm32")]
pub mod web_audio;
