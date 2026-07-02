//! [`SessionRecord`] — the recorded artifacts of a play session, plus its two
//! artifact kinds [`RecordedInput`] and [`RecordedSnapshot`].
//!
//! A session record is what makes a session replayable: the [`LaunchSpec`] it ran
//! from, the ordered input artifacts it consumed, and the ordered snapshot-hash
//! artifacts it produced. The workspace stores opaque, canonical artifacts — it
//! does not interpret inputs or reconstruct snapshots; those are the runtime's
//! job. Order is preserved exactly, because replay depends on it.

use axiom_kernel::{StableHash, Tick};

use crate::launch_spec::LaunchSpec;

/// One recorded input artifact: an opaque input code stamped with the tick it was
/// consumed on. The workspace does not know what the code means; it preserves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedInput {
    tick: Tick,
    input_code: u32,
}

impl RecordedInput {
    /// A recorded input at a tick.
    #[must_use]
    pub fn new(tick: Tick, input_code: u32) -> Self {
        RecordedInput { tick, input_code }
    }

    /// The tick this input was consumed on.
    #[must_use]
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// The opaque input code.
    #[must_use]
    pub fn input_code(&self) -> u32 {
        self.input_code
    }
}

/// One recorded snapshot artifact: a [`StableHash`] over opaque snapshot bytes,
/// stamped with the tick the snapshot was taken at. The hash is a diagnostic
/// index, never the proof — byte equality remains the verdict at replay time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedSnapshot {
    tick: Tick,
    hash: StableHash,
}

impl RecordedSnapshot {
    /// A recorded snapshot from an already-computed hash.
    #[must_use]
    pub fn new(tick: Tick, hash: StableHash) -> Self {
        RecordedSnapshot { tick, hash }
    }

    /// A recorded snapshot, hashing opaque snapshot bytes at a tick.
    #[must_use]
    pub fn of_bytes(tick: Tick, snapshot_bytes: &[u8]) -> Self {
        RecordedSnapshot {
            tick,
            hash: StableHash::of_bytes(snapshot_bytes),
        }
    }

    /// The tick this snapshot was taken at.
    #[must_use]
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// The snapshot-bytes digest.
    #[must_use]
    pub fn hash(&self) -> StableHash {
        self.hash
    }
}

/// The recorded, replayable artifacts of one play session.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRecord {
    record_id: String,
    launch_spec: LaunchSpec,
    inputs: Vec<RecordedInput>,
    snapshots: Vec<RecordedSnapshot>,
}

impl SessionRecord {
    /// Open an empty record over a session's launch spec, identified by
    /// `record_id`.
    #[must_use]
    pub fn new(record_id: &str, launch_spec: LaunchSpec) -> Self {
        SessionRecord {
            record_id: record_id.to_string(),
            launch_spec,
            inputs: Vec::new(),
            snapshots: Vec::new(),
        }
    }

    /// The stable identity of this record. A [`crate::replay_request::ReplayRequest`]
    /// references it to name what to replay.
    #[must_use]
    pub fn record_id(&self) -> &str {
        &self.record_id
    }

    /// The launch spec this record was captured from.
    #[must_use]
    pub fn launch_spec(&self) -> &LaunchSpec {
        &self.launch_spec
    }

    /// Append an input artifact. Insertion order is preserved exactly.
    pub fn record_input(&mut self, input: RecordedInput) {
        self.inputs.push(input);
    }

    /// Append a snapshot artifact. Insertion order is preserved exactly.
    pub fn record_snapshot(&mut self, snapshot: RecordedSnapshot) {
        self.snapshots.push(snapshot);
    }

    /// The recorded inputs, in the order they were recorded.
    #[must_use]
    pub fn inputs(&self) -> &[RecordedInput] {
        &self.inputs
    }

    /// The recorded snapshots, in the order they were recorded.
    #[must_use]
    pub fn snapshots(&self) -> &[RecordedSnapshot] {
        &self.snapshots
    }

    /// A deterministic digest over the record's launch identity and its ordered
    /// artifacts. Order-sensitive: reordering inputs or snapshots changes it.
    #[must_use]
    pub fn digest(&self) -> StableHash {
        let mut words = Vec::with_capacity(1 + self.inputs.len() * 2 + self.snapshots.len() * 2);
        words.push(self.launch_spec.identity().raw());
        self.inputs.iter().for_each(|input| {
            words.push(input.tick().raw());
            words.push(u64::from(input.input_code()));
        });
        self.snapshots.iter().for_each(|snapshot| {
            words.push(snapshot.tick().raw());
            words.push(snapshot.hash().raw());
        });
        StableHash::of_words(&words)
    }
}
