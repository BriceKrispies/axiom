//! Stable status codes, intent-rejection reason codes, and the worker/protocol
//! version constants for the Tier-B worker-control C ABI.
//!
//! Every `extern "C"` entry point returns one of the `STATUS_*` `i32` codes; an
//! intent submission additionally yields a `REASON_*` `u32`. No call ever
//! signals failure by panicking — a panic that escapes the implementation is
//! caught at the ABI boundary and surfaces as [`STATUS_ERR_PANIC`].

// --- call status codes ---

/// The operation succeeded.
pub const STATUS_OK: i32 = 0;
/// The sim handle pointer was null.
pub const STATUS_ERR_NULL_HANDLE: i32 = 1;
/// An argument was invalid — e.g. a null output pointer, or a null input buffer
/// with a non-zero length.
pub const STATUS_ERR_INVALID_ARG: i32 = 2;
/// A caller-provided output buffer was too small. Query the required length and
/// retry; no bytes were written.
pub const STATUS_ERR_BUFFER_TOO_SMALL: i32 = 3;
/// The submitted intent was rejected by validation; the out reason code carries
/// the specific `REASON_*`.
pub const STATUS_REJECTED: i32 = 4;
/// Provided bytes failed to deserialize (replay verification / snapshot restore).
pub const STATUS_ERR_DESERIALIZE: i32 = 5;
/// The engine returned a deterministic error (e.g. `restore_sim` on bad bytes).
pub const STATUS_ERR_ENGINE: i32 = 6;
/// The implementation panicked and was caught at the ABI boundary. The process
/// is intact; the call had no effect beyond recording the panic in last-error.
pub const STATUS_ERR_PANIC: i32 = 7;

// --- intent rejection reason codes ---
//
// 0..=3 deliberately mirror `axiom-net-protocol`'s reason codes so the .NET host
// can forward a worker rejection as a Tier-A `RejectedIntent` without a lookup
// table. 4.. are worker-specific and the host maps them to the closest wire
// reason when echoing to the browser.

/// Not a rejection — the intent was accepted.
pub const REASON_NONE: u32 = 0;
/// The intent payload was malformed (wrong length / undecodable).
pub const REASON_MALFORMED: u32 = 1;
/// The client sequence arrived out of order (older than the last accepted).
pub const REASON_OUT_OF_ORDER: u32 = 2;
/// The player is not a member of this room. (Enforced by the .NET host; the
/// worker enforces [`REASON_INVALID_PLAYER`] for an out-of-range slot.)
pub const REASON_NOT_IN_ROOM: u32 = 3;
/// The client sequence duplicates the last accepted sequence for that player.
pub const REASON_DUPLICATE_SEQUENCE: u32 = 4;
/// The payload exceeded the maximum opaque-payload length.
pub const REASON_PAYLOAD_TOO_LARGE: u32 = 5;
/// The player id is outside `[0, max_players)`.
pub const REASON_INVALID_PLAYER: u32 = 6;
/// Too many intents for one player in a single tick (action-spam / rate limit).
pub const REASON_RATE_LIMITED: u32 = 7;
/// The move the intent implies is illegal for the ruleset (e.g. a teleport).
pub const REASON_IMPOSSIBLE_MOVEMENT: u32 = 8;

// --- version handshake ---

/// Worker semantic version (major). Bumped on an incompatible ABI/behaviour change.
pub const WORKER_VERSION_MAJOR: u32 = 0;
/// Worker semantic version (minor).
pub const WORKER_VERSION_MINOR: u32 = 2;
/// Worker semantic version (patch).
pub const WORKER_VERSION_PATCH: u32 = 0;
/// The Tier-B worker-control protocol version the .NET host must match.
pub const WORKER_PROTOCOL_VERSION: u32 = 1;
