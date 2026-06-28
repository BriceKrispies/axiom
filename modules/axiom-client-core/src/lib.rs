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
//! - the **pending-intent queue** — the `(sequence, payload)` pairs sent but not
//!   yet acknowledged, in insertion order — drained when a snapshot acknowledges
//!   them or a rejection removes one. Retaining the payloads makes the queue the
//!   **resimulation cursor**: `unacked_intents` is exactly the ordered intents to
//!   replay on top of a freshly-snapped authoritative snapshot;
//! - the **interpolation cursor** (`interpolation_tick`) — the latest
//!   authoritative tick set back by a presentation delay, saturating at 0.
//!
//! The simulation itself still runs elsewhere: this module performs **no
//! prediction step and no rollback**. It only holds the reconciliation
//! bookkeeping — which intents to replay, and which past tick to interpolate
//! toward — so a predicted client built on top of it has the exact cursor it
//! needs without this module ever touching game state.
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
