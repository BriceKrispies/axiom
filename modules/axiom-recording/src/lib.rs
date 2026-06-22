//! `axiom-recording` — a deterministic, memory-bounded frame recorder and
//! scrubber.
//!
//! This isolated engine module records per-frame artifacts as **opaque canonical
//! bytes** indexed by kernel [`axiom_kernel::FrameIndex`] / [`axiom_kernel::Tick`],
//! keeps them in a bounded ring buffer, lets a caller scrub/step through retained
//! frames without mutating the live timeline, and proves replay determinism by
//! comparing two recordings byte-for-byte.
//!
//! It is deliberately ignorant of *what* the bytes are: input, runtime-step,
//! state-snapshot and render-command payloads are all undifferentiated `Vec<u8>`.
//! The module never decodes them, never touches a renderer/scene/GPU/browser API,
//! never reads wall-clock time, and uses no randomness or global mutable state —
//! recording and comparison are pure functions of the bytes handed in. Pixel
//! buffers, screenshots, video, on-disk persistence and fork-from-frame are all
//! out of scope.
//!
//! # Public surface
//!
//! The module exposes exactly one facade, [`RecordingApi`]. Every other type
//! (the captures, the timeline, the playback mode, the determinism report, the
//! artifact kind) is returned *opaquely* through that facade and read via its
//! public accessors; none of them is named in this module's public API.

mod artifact_kind;
mod determinism_report;
mod error;
mod frame_capture;
mod frame_timeline;
mod hash;
mod recording_api;
mod timeline_mode;

pub use recording_api::RecordingApi;
