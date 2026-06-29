//! # Axiom Audio — Engine Module (deterministic audio bookkeeping)
//!
//! The deterministic half of sound: the part that owns *what* plays and *when*
//! (on the audio clock), with no `AudioContext` or oscillator in sight. It
//! allocates handles, validates + clamps a synthesis [`ToneSpec`], folds master
//! volume and mute into one effective gain, and accumulates a `ScheduledBatch`
//! a compiled-out `wasm32` arm realizes into real Web Audio nodes. Every decision
//! the mixer makes stays here, on the native-testable side — the same two-arm
//! shape as `axiom-windowing`.
//!
//! ## What this module is
//! - The single owner of audio *scheduling/mix bookkeeping*: sample playback,
//!   music playlists, tone synthesis, audio-clock scheduling, and master
//!   volume/mute, all expressed as data.
//! - Branchless and 100% covered on native: it is pure bookkeeping (allocate a
//!   [`VoiceId`], validate a [`ToneSpec`] by table-indexed wave dispatch, fold a
//!   `set_master_volume` into mix state, accumulate the command batch).
//!
//! ## What this module is not
//! Not a synthesizer, not an audio renderer, and — in this rlib — not a Web Audio
//! binding. It composes no other module: it builds on the kernel alone
//! (`HandleId` for handles, `Ratio` for 0..1 volume/sustain/depth). The real
//! `AudioContext` arm is a `#[cfg(target_arch = "wasm32")]` platform arm behind
//! this core (a sanctioned `PLATFORM_FACING_MODULES` entry, Module Law #9).
//!
//! ## Determinism (§6)
//! Audio is presentation-only. Data flows **only** sim/app -> core -> arm: the
//! core exposes no getter a fixed update could sample (no "is playing", no
//! "position"). A `ScheduledBatch` is consumed by the platform arm and never
//! read back into a sim. The core's determinism is "same calls => same batch".
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioral facade, [`AudioApi`], plus the one
//! identity-vocabulary line (`pub use ids::{…}`) naming the value-type nouns a
//! caller constructs to drive it.

mod audio_api;
mod ids;

pub use audio_api::AudioApi;
pub use ids::{
    AnalyserId, AudioInput, AudioSeconds, Band, BandLayout, Envelope, Hertz, Lfo, MusicOpts,
    PlayOpts, SoundId, ToneSpec, VoiceId, Wave,
};
