//! # Axiom Netplay FFI — the deterministic simulation worker
//!
//! This crate compiles to a native shared library (`cdylib`) so the .NET 10
//! authoritative server can run the **real Axiom engine** in-process via
//! P/Invoke, with no WASM involved. WASM is only the browser shipping format;
//! here the same engine is compiled native and driven headlessly as the
//! authority.
//!
//! ## Two tiers, kept separate
//! - **Tier A — browser wire protocol** ([`codec`]): C-ABI exports of the
//!   canonical [`axiom_net_protocol`] codec, so the host has one source of truth
//!   for the browser-facing frames (`JoinRoom` / `ClientIntent` / `Welcome` /
//!   `ServerSnapshot` / `RejectedIntent` …). The browser can encode only these.
//! - **Tier B — worker-control protocol** ([`ffi`]): the server-only surface the
//!   .NET host drives — create / load / submit / advance / snapshot / hash /
//!   export-replay / verify-replay, plus a version handshake. Nothing here is
//!   reachable from a socket, so a browser can never submit state.
//!
//! ## Authority
//! Authoritative state is the engine's durable scene state, read through
//! [`session::Session`] (`snapshot_sim()` / `restore_sim()`). There is **no**
//! parallel state mirror. Per-tick state hashes come from canonical snapshot
//! bytes ([`axiom_kernel::StableHash`]); byte-equality of those bytes is the
//! determinism proof. Intent validation, replay capture, and replay verification
//! all live here in the app — the game-specific schema (the [`ruleset`]) lives
//! only in this app, never in a protocol module or layer.
//!
//! Every `extern "C"` entry point is panic-guarded: no Rust panic may unwind
//! across the C ABI (see [`ffi`]).

/// Tier-A: C-ABI exports of the browser-facing `axiom-net-protocol` wire codec.
mod codec;

/// Tier-B: the panic-guarded worker-control C ABI the .NET host drives.
pub mod ffi;
/// The out-of-process worker-control protocol (the IPC twin of [`ffi`]), used by
/// the `axiom-netplay-worker` binary when the host runs the sim out-of-process.
pub mod ipc;
/// Deterministic replay records: capture, canonical codec, and verification.
pub mod replay;
/// The v1 game ruleset — the only place game-specific schema lives.
pub mod ruleset;
/// The safe authoritative simulation session the C ABI wraps.
pub mod session;
/// Stable status / rejection-reason / version constants for the C ABI.
pub mod status;

pub use session::Session;
