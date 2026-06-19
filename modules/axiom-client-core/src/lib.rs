//! # Axiom Client Core — Engine Module (the portable client state machine)
//!
//! `axiom-client-core` owns the **portable client-side multiplayer state
//! machine**. It opens no socket, knows nothing about the browser, and does not
//! speak the wire format. It consumes the authoritative values a server sends
//! and produces the outbound intents a client sends — pure deterministic logic
//! over plain primitives.
//!
//! ## The model
//! The **server is authoritative**. A client sends *intents* (never state) and
//! receives authoritative *snapshots*. This module tracks exactly the state a
//! client needs to participate honestly:
//!
//! - the **connection state** (`Disconnected → Connecting → Connected`);
//! - a **monotonically increasing client sequence**, starting at `1`, assigned
//!   to each outbound intent;
//! - the **latest authoritative server tick**, advanced only by snapshots and
//!   never allowed to go backwards;
//! - the **pending-intent queue** — the sequences sent but not yet acknowledged,
//!   in insertion order — drained when a snapshot acknowledges them or a
//!   rejection removes one.
//!
//! There is deliberately **no prediction and no rollback** here: tracking
//! pending intents is the whole of the first version's client logic.
//!
//! ## Why the boundary is plain primitives
//! An engine module may never depend on another module, so this module does not
//! depend on `axiom-net-protocol`. It neither decodes nor encodes wire frames;
//! it accepts and returns plain values (`u64` / `&[u8]` / `Vec<u8>` / `bool` /
//! `Option`). The app (or the TypeScript package) decodes a frame with
//! `axiom-net-protocol`, feeds the values here, and encodes the intent this
//! module produces. State-machine rejections are modelled as `false` / `None`
//! (an operation that does not apply in the current state), not as errors —
//! they are normal, expected outcomes, not failures.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`ClientCoreApi`].

mod client_core_api;
mod connection_state;

pub use client_core_api::ClientCoreApi;
