//! # Axiom Net Protocol — Engine Module (the multiplayer wire contract)
//!
//! `axiom-net-protocol` owns the **stable multiplayer message contract**: the
//! exact bytes clients and servers exchange. It is *protocol data only* — no
//! session state, no socket, no browser. The portable client state machine
//! lives in `axiom-client-core`; the transport lives at the app/package edge.
//!
//! ## The model
//! Multiplayer in Axiom keeps the **server authoritative**. Clients send
//! *intents* (what they want to do), never state; the server replies with
//! authoritative *snapshots* and *events*. This module encodes that asymmetry
//! as two message families over a single versioned frame format:
//!
//! ```text
//! client → server :  JoinRoom · LeaveRoom · ClientIntent
//! server → client :  Welcome · ServerSnapshot · ServerEvent · RejectedIntent
//! ```
//!
//! ## Wire format
//! Every frame is `SchemaVersion (major.minor)` then a one-byte **message kind**
//! discriminant, then the message body — all little-endian via the kernel's
//! [`axiom_kernel::BinaryWriter`] / [`axiom_kernel::BinaryReader`], so the bytes
//! are byte-identical on every platform (native and wasm). Decoding is fully
//! bounds-checked: an incompatible major, an unknown kind, a truncated body, or
//! an over-sized payload each fails with a precise [`axiom_kernel::KernelError`]
//! rather than panicking. There is no hash-map iteration, no random id, and no
//! wall-clock timestamp anywhere in the format.
//!
//! ## Opaque payloads
//! Intent / snapshot / event payloads are **opaque bounded byte buffers** today
//! (`MAX_PAYLOAD_LEN`). The protocol does not interpret them — a future schema
//! layer will. Room ids are likewise opaque but bounded and non-empty
//! (`MAX_ROOM_ID_LEN`). This keeps the contract free of game-specific schema.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`NetProtocolApi`]. Because a module
//! exposes only one nameable type, every message and field type is internal and
//! crosses the facade boundary as plain primitives (`u32` / `u64` / `&[u8]` /
//! `Vec<u8>`) — which is also exactly a wire shape, so an app or the TypeScript
//! package can own the socket without naming a protocol type.

mod acks;
mod client_id;
mod client_intent;
mod client_intent_for;
mod frame;
mod join_room;
mod leave_room;
mod net_protocol_api;
mod opaque_payload;
mod protocol_version;
mod rejected_intent;
mod room_id;
mod server_event;
mod server_snapshot;
mod server_snapshot_for;
mod server_snapshot_for_delta;
mod snapshot_delta;
mod welcome;

pub use net_protocol_api::NetProtocolApi;
