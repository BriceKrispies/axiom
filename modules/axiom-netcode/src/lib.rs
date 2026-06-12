//! # Axiom Netcode — Engine Module (deterministic-lockstep networking core)
//!
//! Axiom's simulation is deterministic and replayable-by-tick: the same tick
//! replayed twice produces byte-identical state. Multiplayer over such a sim is
//! **deterministic lockstep** — every peer runs the *same* sim, only **inputs**
//! cross the wire (never state), and a tick advances once every peer's input
//! for it is present. "Server" and "client" run identical code; the server is
//! just the peer that also arbitrates input order and referees desync.
//!
//! This module owns the deterministic **session**, over plain data:
//!
//! ```text
//! local input  -> submit_local -> wire bytes ──(app's socket)──> peers
//! peer bytes ──(app's socket)──> ingest -> input timeline
//! when every peer's input for the next tick is present:
//!     confirm_tick -> ordered inputs -> (app ticks its sim) -> state hash
//!     record_local_hash / reconcile -> InSync | Desync
//! ```
//!
//! ## What this module is
//! - The input timeline keyed by `(tick, peer)` in stable order.
//! - The lockstep readiness gate and the confirmed-tick cursor.
//! - The wire codec (a versioned, length-checked byte format).
//! - State-hash reconciliation: a desync referee over per-peer hashes.
//!
//! ## What this module is not
//! Not a socket and not a transport. The boundary is **plain bytes**: the app
//! owns the socket loop (the nondeterministic edge) and calls `submit_local` /
//! `ingest` with byte messages, exactly as a wire would. It composes no other
//! module, drives no `App` (an engine module may not depend on one), reads no
//! clock, and holds no global state. A real `wasm32` transport is a later,
//! separate platform-facing slice.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`NetcodeApi`]. The session it
//! drives and the status it reports are reached only through it.

mod digest;
mod input_timeline;
mod net_command;
mod net_message;
mod netcode_api;
mod peer_id;
mod session;
mod sync_status;

pub use netcode_api::NetcodeApi;
