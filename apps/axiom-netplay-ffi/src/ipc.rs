//! The out-of-process worker control protocol.
//!
//! When the host runs the sim worker as a separate OS process (rather than
//! in-process via the [`crate::ffi`] C ABI), it talks to it over a local socket
//! using this tiny request/response protocol. Each request and response is one
//! length-prefixed frame; the body layout is little-endian and documented per
//! variant below. This is the IPC twin of the Tier-B C ABI — the same `Session`
//! operations, marshalled as bytes instead of pointers.
//!
//! The .NET host mirrors this exact layout in `OutOfProcAxiomSim`. The cross-check
//! is behavioural: an out-of-process worker driven with the same inputs as an
//! in-process `Session` must produce byte-identical snapshots and hashes (proven
//! by `tests/worker_process.rs` and the host's parity test).
//!
//! This module is part of the worker **app**, not the engine spine — it is exempt
//! from the branchless/coverage laws, but is still unit-tested here and end-to-end
//! across a real process boundary.

use crate::session::Session;

// Request opcodes. A response is tagged with the same opcode it answers, so a
// response frame is self-describing and round-trippable.
const OP_SUBMIT_INTENT: u8 = 0x01;
const OP_ADVANCE_TICK: u8 = 0x02;
const OP_RENDER_VIEW: u8 = 0x03;
const OP_SNAPSHOT: u8 = 0x04;
const OP_LOAD_STATE: u8 = 0x05;
const OP_STATE_HASH: u8 = 0x06;
const OP_EXPORT_REPLAY: u8 = 0x07;
const OP_RESTORE_AT: u8 = 0x08;
const TAG_MALFORMED: u8 = 0xFF;

/// A host→worker request: one `Session` operation.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// `[u32 player][u64 sequence][u64 predicted_tick][u32 len][len bytes]`
    SubmitIntent {
        player: u32,
        sequence: u64,
        predicted_tick: u64,
        payload: Vec<u8>,
    },
    /// `[u64 target]` (target is informational; the session advances one tick).
    AdvanceTick { target: u64 },
    /// (no body)
    RenderView,
    /// (no body)
    Snapshot,
    /// `[u32 len][len bytes]` — restore engine state only (leaves the tick).
    LoadState { bytes: Vec<u8> },
    /// `[u64 tick][u32 len][len bytes]` — restore engine state AND re-establish
    /// the tick (crash recovery).
    RestoreAt { tick: u64, bytes: Vec<u8> },
    /// (no body)
    StateHash,
    /// (no body)
    ExportReplay,
}

/// A worker→host response.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// The `REASON_*` code from `submit_intent` (0 = accepted).
    Reason(u32),
    /// The new tick count and authoritative state hash.
    Tick { tick: u64, hash: u64 },
    /// The authoritative render view (each player's x,y).
    RenderView(Vec<f32>),
    /// The canonical snapshot bytes and their state hash.
    Snapshot { hash: u64, bytes: Vec<u8> },
    /// Whether a `LoadState` succeeded.
    Loaded(bool),
    /// The current authoritative state hash.
    StateHash(u64),
    /// The canonical replay-record bytes.
    Replay(Vec<u8>),
    /// The request frame could not be decoded.
    Malformed,
}

impl Request {
    /// Encode this request into a frame body (the host frames it with a u32 length
    /// prefix on the wire).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Request::SubmitIntent {
                player,
                sequence,
                predicted_tick,
                payload,
            } => {
                out.push(OP_SUBMIT_INTENT);
                out.extend_from_slice(&player.to_le_bytes());
                out.extend_from_slice(&sequence.to_le_bytes());
                out.extend_from_slice(&predicted_tick.to_le_bytes());
                push_bytes(&mut out, payload);
            }
            Request::AdvanceTick { target } => {
                out.push(OP_ADVANCE_TICK);
                out.extend_from_slice(&target.to_le_bytes());
            }
            Request::RenderView => out.push(OP_RENDER_VIEW),
            Request::Snapshot => out.push(OP_SNAPSHOT),
            Request::LoadState { bytes } => {
                out.push(OP_LOAD_STATE);
                push_bytes(&mut out, bytes);
            }
            Request::RestoreAt { tick, bytes } => {
                out.push(OP_RESTORE_AT);
                out.extend_from_slice(&tick.to_le_bytes());
                push_bytes(&mut out, bytes);
            }
            Request::StateHash => out.push(OP_STATE_HASH),
            Request::ExportReplay => out.push(OP_EXPORT_REPLAY),
        }
        out
    }

    /// Decode a request frame, or `None` if it is malformed.
    pub fn decode(frame: &[u8]) -> Option<Request> {
        let mut c = Cursor::new(frame);
        match c.u8()? {
            OP_SUBMIT_INTENT => Some(Request::SubmitIntent {
                player: c.u32()?,
                sequence: c.u64()?,
                predicted_tick: c.u64()?,
                payload: c.lp_bytes()?,
            }),
            OP_ADVANCE_TICK => Some(Request::AdvanceTick { target: c.u64()? }),
            OP_RENDER_VIEW => Some(Request::RenderView),
            OP_SNAPSHOT => Some(Request::Snapshot),
            OP_LOAD_STATE => Some(Request::LoadState {
                bytes: c.lp_bytes()?,
            }),
            OP_RESTORE_AT => Some(Request::RestoreAt {
                tick: c.u64()?,
                bytes: c.lp_bytes()?,
            }),
            OP_STATE_HASH => Some(Request::StateHash),
            OP_EXPORT_REPLAY => Some(Request::ExportReplay),
            _ => None,
        }
    }
}

impl Response {
    /// Encode this response into a frame body.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Response::Reason(reason) => {
                out.push(OP_SUBMIT_INTENT);
                out.extend_from_slice(&reason.to_le_bytes());
            }
            Response::Tick { tick, hash } => {
                out.push(OP_ADVANCE_TICK);
                out.extend_from_slice(&tick.to_le_bytes());
                out.extend_from_slice(&hash.to_le_bytes());
            }
            Response::RenderView(floats) => {
                out.push(OP_RENDER_VIEW);
                out.extend_from_slice(&(floats.len() as u32).to_le_bytes());
                floats
                    .iter()
                    .for_each(|f| out.extend_from_slice(&f.to_le_bytes()));
            }
            Response::Snapshot { hash, bytes } => {
                out.push(OP_SNAPSHOT);
                out.extend_from_slice(&hash.to_le_bytes());
                push_bytes(&mut out, bytes);
            }
            Response::Loaded(ok) => {
                out.push(OP_LOAD_STATE);
                out.push(u8::from(*ok));
            }
            Response::StateHash(hash) => {
                out.push(OP_STATE_HASH);
                out.extend_from_slice(&hash.to_le_bytes());
            }
            Response::Replay(bytes) => {
                out.push(OP_EXPORT_REPLAY);
                push_bytes(&mut out, bytes);
            }
            Response::Malformed => out.push(TAG_MALFORMED),
        }
        out
    }

    /// Decode a response frame, or `None` if it is malformed.
    pub fn decode(frame: &[u8]) -> Option<Response> {
        let mut c = Cursor::new(frame);
        match c.u8()? {
            OP_SUBMIT_INTENT => Some(Response::Reason(c.u32()?)),
            OP_ADVANCE_TICK => Some(Response::Tick {
                tick: c.u64()?,
                hash: c.u64()?,
            }),
            OP_RENDER_VIEW => {
                let count = c.u32()? as usize;
                let mut floats = Vec::with_capacity(count);
                for _ in 0..count {
                    floats.push(c.f32()?);
                }
                Some(Response::RenderView(floats))
            }
            OP_SNAPSHOT => Some(Response::Snapshot {
                hash: c.u64()?,
                bytes: c.lp_bytes()?,
            }),
            OP_LOAD_STATE => Some(Response::Loaded(c.u8()? != 0)),
            OP_STATE_HASH => Some(Response::StateHash(c.u64()?)),
            OP_EXPORT_REPLAY => Some(Response::Replay(c.lp_bytes()?)),
            TAG_MALFORMED => Some(Response::Malformed),
            _ => None,
        }
    }
}

/// Apply one request to the session and produce its response.
pub fn dispatch(session: &mut Session, request: Request) -> Response {
    match request {
        Request::SubmitIntent {
            player,
            sequence,
            predicted_tick,
            payload,
        } => Response::Reason(session.submit_intent(player, sequence, predicted_tick, &payload)),
        Request::AdvanceTick { target: _ } => {
            let (tick, hash) = session.advance();
            Response::Tick { tick, hash }
        }
        Request::RenderView => Response::RenderView(session.render_view()),
        Request::Snapshot => Response::Snapshot {
            hash: session.state_hash(),
            bytes: session.snapshot(),
        },
        Request::LoadState { bytes } => Response::Loaded(session.restore(&bytes)),
        Request::RestoreAt { tick, bytes } => Response::Loaded(session.restore_at(tick, &bytes)),
        Request::StateHash => Response::StateHash(session.state_hash()),
        Request::ExportReplay => Response::Replay(session.export_replay()),
    }
}

/// Decode a request frame, apply it, and return the encoded response frame body.
/// A frame that does not decode yields a `Malformed` response rather than a panic
/// or a dropped connection.
pub fn handle(session: &mut Session, request_frame: &[u8]) -> Vec<u8> {
    match Request::decode(request_frame) {
        Some(request) => dispatch(session, request).encode(),
        None => Response::Malformed.encode(),
    }
}

/// Append a u32-length-prefixed byte block.
fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// A little-endian byte cursor that returns `None` on underflow.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Cursor { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Option<u64> {
        self.take(8)
            .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    fn f32(&mut self) -> Option<f32> {
        self.take(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn lp_bytes(&mut self) -> Option<Vec<u8>> {
        let len = self.u32()? as usize;
        self.take(len).map(<[u8]>::to_vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ruleset;

    fn session() -> Session {
        Session::new(7, 2, 16_666_667)
    }

    #[test]
    fn requests_round_trip() {
        let cases = vec![
            Request::SubmitIntent {
                player: 1,
                sequence: 9,
                predicted_tick: 3,
                payload: vec![1, 2, 3, 4, 5, 6, 7, 8],
            },
            Request::AdvanceTick { target: 42 },
            Request::RenderView,
            Request::Snapshot,
            Request::LoadState {
                bytes: vec![9, 8, 7],
            },
            Request::RestoreAt {
                tick: 12,
                bytes: vec![5, 4, 3],
            },
            Request::StateHash,
            Request::ExportReplay,
        ];
        for case in cases {
            assert_eq!(Request::decode(&case.encode()), Some(case));
        }
    }

    #[test]
    fn responses_round_trip() {
        let cases = vec![
            Response::Reason(7),
            Response::Tick {
                tick: 5,
                hash: 0xDEAD_BEEF,
            },
            Response::RenderView(vec![-1.5, 0.0, 1.5, 0.0]),
            Response::Snapshot {
                hash: 11,
                bytes: vec![4, 5, 6],
            },
            Response::Loaded(true),
            Response::Loaded(false),
            Response::StateHash(123),
            Response::Replay(vec![1, 2, 3, 4]),
            Response::Malformed,
        ];
        for case in cases {
            assert_eq!(Response::decode(&case.encode()), Some(case));
        }
    }

    #[test]
    fn a_truncated_frame_decodes_to_none() {
        assert_eq!(Request::decode(&[]), None);
        assert_eq!(Request::decode(&[OP_ADVANCE_TICK, 1, 2]), None); // u64 underflow
        assert_eq!(Request::decode(&[0x55]), None); // unknown opcode
        assert_eq!(Response::decode(&[]), None);
        assert_eq!(Response::decode(&[0x55]), None);
    }

    #[test]
    fn dispatch_submits_advances_and_reports_state() {
        let mut s = session();
        let payload = ruleset::encode_move(0.5, 0.0);
        assert_eq!(
            dispatch(
                &mut s,
                Request::SubmitIntent {
                    player: 0,
                    sequence: 1,
                    predicted_tick: 0,
                    payload,
                }
            ),
            Response::Reason(0)
        );
        let advanced = dispatch(&mut s, Request::AdvanceTick { target: 0 });
        assert!(matches!(advanced, Response::Tick { tick: 1, .. }));

        // The render view reflects the applied move (player 0 moved right from -1.5).
        match dispatch(&mut s, Request::RenderView) {
            Response::RenderView(v) => assert_eq!(v, vec![-1.0, 0.0, 1.5, 0.0]),
            other => panic!("expected RenderView, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_snapshot_load_round_trips_state() {
        let mut s = session();
        dispatch(
            &mut s,
            Request::SubmitIntent {
                player: 1,
                sequence: 1,
                predicted_tick: 0,
                payload: ruleset::encode_move(0.0, 0.5),
            },
        );
        dispatch(&mut s, Request::AdvanceTick { target: 0 });
        let (bytes, hash) = match dispatch(&mut s, Request::Snapshot) {
            Response::Snapshot { hash, bytes } => (bytes, hash),
            other => panic!("expected Snapshot, got {other:?}"),
        };

        let mut fresh = session();
        assert_eq!(
            dispatch(&mut fresh, Request::LoadState { bytes }),
            Response::Loaded(true)
        );
        assert_eq!(
            dispatch(&mut fresh, Request::StateHash),
            Response::StateHash(hash)
        );
    }

    #[test]
    fn restore_at_reestablishes_engine_state_and_tick() {
        // Advance a session a few ticks, snapshot it, then restore those bytes AT
        // a chosen tick into a fresh session: both the state hash and the tick must
        // resume, so the next advance continues the deterministic sequence.
        let mut s = session();
        dispatch(
            &mut s,
            Request::SubmitIntent {
                player: 0,
                sequence: 1,
                predicted_tick: 0,
                payload: ruleset::encode_move(0.3, 0.0),
            },
        );
        (0..4).for_each(|t| {
            dispatch(&mut s, Request::AdvanceTick { target: t });
        });
        let (bytes, hash) = match dispatch(&mut s, Request::Snapshot) {
            Response::Snapshot { hash, bytes } => (bytes, hash),
            other => panic!("expected Snapshot, got {other:?}"),
        };

        let mut fresh = session();
        assert_eq!(
            dispatch(&mut fresh, Request::RestoreAt { tick: 4, bytes }),
            Response::Loaded(true)
        );
        assert_eq!(
            dispatch(&mut fresh, Request::StateHash),
            Response::StateHash(hash)
        );
        // The tick resumed at 4, so the next advance yields tick 5 (not 1).
        assert_eq!(
            dispatch(&mut fresh, Request::AdvanceTick { target: 5 }),
            dispatch(&mut s, Request::AdvanceTick { target: 5 })
        );
    }

    #[test]
    fn handle_decodes_dispatches_and_flags_malformed() {
        let mut s = session();
        let frame = Request::StateHash.encode();
        let response = Response::decode(&handle(&mut s, &frame)).unwrap();
        assert!(matches!(response, Response::StateHash(_)));

        // A garbage frame yields a Malformed response, never a panic.
        assert_eq!(
            Response::decode(&handle(&mut s, &[0x99, 0x99])),
            Some(Response::Malformed)
        );
    }

    #[test]
    fn export_replay_dispatches() {
        let mut s = session();
        dispatch(&mut s, Request::AdvanceTick { target: 0 });
        match dispatch(&mut s, Request::ExportReplay) {
            Response::Replay(bytes) => assert!(!bytes.is_empty()),
            other => panic!("expected Replay, got {other:?}"),
        }
    }
}
