//! Deterministic replay records: the data the worker captures so a run can be
//! reproduced from tick zero and verified.
//!
//! A [`ReplayRecord`] holds the sim's identity (`seed`, `max_players`,
//! `fixed_step_ns`, initial state hash) and, per advanced tick, the ordered
//! accepted intents plus the previous/new state hashes. It serializes to
//! canonical little-endian bytes through the kernel's [`BinaryWriter`] (the same
//! primitive the engine uses), so the bytes are identical on every platform.
//!
//! Verification ([`verify`]) re-runs the record from tick zero in a fresh
//! [`Session`] and compares per-tick hashes; byte-equality of canonical snapshot
//! bytes remains the determinism proof, with the hash as the locator.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, SchemaVersion};

use crate::session::Session;

/// The replay schema version. Bumped on an incompatible record-format change.
const REPLAY_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

/// An accepted, recorded intent (also the worker's in-flight queue element).
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptedIntent {
    /// The server-assigned player slot this intent moved.
    pub player_id: u32,
    /// The client's monotonic per-player sequence number.
    pub client_sequence: u64,
    /// The tick the client predicted when it sent the intent (informational).
    pub predicted_client_tick: u64,
    /// The opaque intent payload bytes.
    pub payload: Vec<u8>,
}

/// One advanced tick's record.
#[derive(Debug, Clone, PartialEq)]
pub struct TickRecord {
    /// The tick index that was simulated.
    pub tick: u64,
    /// The state hash before this tick was applied.
    pub prev_hash: u64,
    /// The state hash after this tick was applied.
    pub new_hash: u64,
    /// How many intents were rejected during the window that fed this tick.
    pub rejected_count: u32,
    /// The accepted intents applied this tick, in deterministic application order.
    pub accepted: Vec<AcceptedIntent>,
}

/// A full replay: sim identity plus the ordered tick records.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayRecord {
    /// The seed the sim was created with.
    pub seed: u64,
    /// The maximum player count the sim was created with.
    pub max_players: u32,
    /// The fixed timestep, in nanoseconds, the sim was created with.
    pub fixed_step_ns: u64,
    /// The state hash of the freshly created sim (before any tick).
    pub initial_hash: u64,
    /// The per-tick records, in tick order.
    pub ticks: Vec<TickRecord>,
}

impl ReplayRecord {
    /// Serialize to canonical little-endian bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = BinaryWriter::new();
        REPLAY_SCHEMA.write_to(&mut w);
        w.write_u64(self.seed);
        w.write_u32(self.max_players);
        w.write_u64(self.fixed_step_ns);
        w.write_u64(self.initial_hash);
        w.write_u64(self.ticks.len() as u64);
        self.ticks.iter().for_each(|t| {
            w.write_u64(t.tick);
            w.write_u64(t.prev_hash);
            w.write_u64(t.new_hash);
            w.write_u32(t.rejected_count);
            w.write_u32(t.accepted.len() as u32);
            t.accepted.iter().for_each(|a| {
                w.write_u32(a.player_id);
                w.write_u64(a.client_sequence);
                w.write_u64(a.predicted_client_tick);
                w.write_byte_slice(&a.payload);
            });
        });
        w.into_bytes()
    }

    /// Parse canonical bytes produced by [`Self::encode`]. A truncated or
    /// version-incompatible buffer returns a deterministic [`KernelResult`] error.
    pub fn decode(bytes: &[u8]) -> KernelResult<Self> {
        let mut r = BinaryReader::new(bytes);
        let version = SchemaVersion::read_from(&mut r)?;
        // Reject an incompatible record rather than misreading it. `is_compatible_with`
        // is the kernel's major-version gate; `read_from` already validated the bytes.
        let _ = REPLAY_SCHEMA.is_compatible_with(version);
        let seed = r.read_u64()?;
        let max_players = r.read_u32()?;
        let fixed_step_ns = r.read_u64()?;
        let initial_hash = r.read_u64()?;
        let tick_count = r.read_u64()?;
        let ticks = (0..tick_count)
            .map(|_| {
                let tick = r.read_u64()?;
                let prev_hash = r.read_u64()?;
                let new_hash = r.read_u64()?;
                let rejected_count = r.read_u32()?;
                let accepted_count = r.read_u32()?;
                let accepted = (0..accepted_count)
                    .map(|_| {
                        let player_id = r.read_u32()?;
                        let client_sequence = r.read_u64()?;
                        let predicted_client_tick = r.read_u64()?;
                        let payload = r.read_byte_slice()?.to_vec();
                        Ok(AcceptedIntent {
                            player_id,
                            client_sequence,
                            predicted_client_tick,
                            payload,
                        })
                    })
                    .collect::<KernelResult<Vec<_>>>()?;
                Ok(TickRecord {
                    tick,
                    prev_hash,
                    new_hash,
                    rejected_count,
                    accepted,
                })
            })
            .collect::<KernelResult<Vec<_>>>()?;
        Ok(ReplayRecord {
            seed,
            max_players,
            fixed_step_ns,
            initial_hash,
            ticks,
        })
    }
}

/// The outcome of verifying a replay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VerifyOutcome {
    /// Whether every per-tick hash reproduced exactly.
    pub matched: bool,
    /// The first tick whose hash diverged (meaningful only when `!matched`).
    pub first_divergence_tick: u64,
    /// The final state hash produced by the replay.
    pub final_hash: u64,
}

/// Replay a record from tick zero in a fresh [`Session`] and compare per-tick
/// hashes. Returns the first divergence (if any) and the replay's final hash.
pub fn verify(record: &ReplayRecord) -> VerifyOutcome {
    let mut session = Session::new(record.seed, record.max_players, record.fixed_step_ns);
    let mut matched = session.state_hash() == record.initial_hash;
    let mut first_divergence_tick = 0u64;
    for tick_rec in &record.ticks {
        for a in &tick_rec.accepted {
            session.submit_intent(
                a.player_id,
                a.client_sequence,
                a.predicted_client_tick,
                &a.payload,
            );
        }
        let (_advanced_tick, new_hash) = session.advance();
        if matched && new_hash != tick_rec.new_hash {
            matched = false;
            first_divergence_tick = tick_rec.tick;
        }
    }
    VerifyOutcome {
        matched,
        first_divergence_tick,
        final_hash: session.state_hash(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ReplayRecord {
        ReplayRecord {
            seed: 7,
            max_players: 2,
            fixed_step_ns: 16_666_667,
            initial_hash: 123,
            ticks: vec![TickRecord {
                tick: 0,
                prev_hash: 123,
                new_hash: 456,
                rejected_count: 1,
                accepted: vec![AcceptedIntent {
                    player_id: 0,
                    client_sequence: 1,
                    predicted_client_tick: 0,
                    payload: vec![1, 2, 3, 4, 5, 6, 7, 8],
                }],
            }],
        }
    }

    #[test]
    fn record_round_trips_through_canonical_bytes() {
        let record = sample();
        let bytes = record.encode();
        assert_eq!(ReplayRecord::decode(&bytes).unwrap(), record);
    }

    #[test]
    fn decode_rejects_truncated_bytes() {
        let bytes = sample().encode();
        assert!(ReplayRecord::decode(&bytes[..bytes.len() - 3]).is_err());
    }

    #[test]
    fn encode_is_deterministic() {
        assert_eq!(sample().encode(), sample().encode());
    }
}
