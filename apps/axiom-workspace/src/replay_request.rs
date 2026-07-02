//! [`ReplayRequest`] — a request to replay a prior [`SessionRecord`].
//!
//! A replay request is the workspace's contract *to* a future runtime
//! integration: "re-launch this exact spec and re-drive these exact artifacts."
//! It references the record by its identity and pins the record's expected digest
//! so a replayer can detect a record that changed underneath it. The workspace
//! itself replays nothing — it only names what should be replayed.

use axiom_kernel::StableHash;

use crate::launch_spec::LaunchSpec;
use crate::session_record::SessionRecord;

/// A request to replay a previously recorded session.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayRequest {
    record_id: String,
    launch_spec: LaunchSpec,
    expected_digest: StableHash,
}

impl ReplayRequest {
    /// Build a replay request that points at a session record. It copies the
    /// record's identity, its launch spec, and its current digest so a future
    /// replayer can re-launch and validate against the same artifacts.
    #[must_use]
    pub fn for_record(record: &SessionRecord) -> Self {
        ReplayRequest {
            record_id: record.record_id().to_string(),
            launch_spec: record.launch_spec().clone(),
            expected_digest: record.digest(),
        }
    }

    /// The identity of the record this request replays.
    #[must_use]
    pub fn record_id(&self) -> &str {
        &self.record_id
    }

    /// The launch spec to re-launch for the replay.
    #[must_use]
    pub fn launch_spec(&self) -> &LaunchSpec {
        &self.launch_spec
    }

    /// The record digest this request expects at replay time. A replayer compares
    /// it against the record it loads to detect drift.
    #[must_use]
    pub fn expected_digest(&self) -> StableHash {
        self.expected_digest
    }
}
