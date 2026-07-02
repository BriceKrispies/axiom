//! [`PlaySession`] — a launched or launchable real runtime session, and
//! [`PlaySessionStatus`] — its lifecycle state.
//!
//! A play session is a *handle to* a runtime session, not a runtime. It owns the
//! [`LaunchSpec`] it was created from and a coarse lifecycle status; it runs no
//! game rules, steps no simulation, and reads no clock. When real runtime
//! integration lands, driving a session's status transitions becomes the runtime
//! adapter's job — this scaffold only models the states and the launch identity.

use axiom_kernel::StableHash;

use crate::launch_spec::LaunchSpec;

/// The coarse lifecycle state of a [`PlaySession`].
///
/// These are workspace-observation states, not simulation states: the workspace
/// watches a session move through them; it does not compute anything inside them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaySessionStatus {
    /// The session has been created from a launch spec but not yet launched.
    Created,
    /// The workspace has asked the runtime to launch; it is not yet running.
    Launching,
    /// The runtime reports the session is running.
    Running,
    /// The session has ended (cleanly or otherwise).
    Ended,
}

/// A handle to a launchable/launched runtime session.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaySession {
    launch_spec: LaunchSpec,
    status: PlaySessionStatus,
}

impl PlaySession {
    /// Create a session from a launch spec. It begins in
    /// [`PlaySessionStatus::Created`] and simulates nothing.
    #[must_use]
    pub fn created(launch_spec: LaunchSpec) -> Self {
        PlaySession {
            launch_spec,
            status: PlaySessionStatus::Created,
        }
    }

    /// The launch spec this session was created from.
    #[must_use]
    pub fn launch_spec(&self) -> &LaunchSpec {
        &self.launch_spec
    }

    /// The current lifecycle status.
    #[must_use]
    pub fn status(&self) -> PlaySessionStatus {
        self.status
    }

    /// Record an observed lifecycle transition. This does not run the runtime; it
    /// only updates the workspace's view of a session driven elsewhere.
    pub fn observe_status(&mut self, status: PlaySessionStatus) {
        self.status = status;
    }

    /// A deterministic, contract-only summary digest of the session: the launch
    /// identity folded with the current status. Named `summarize_contract` to be
    /// explicit that it summarizes the scaffold contract — it is not a game step.
    #[must_use]
    pub fn summarize_contract(&self) -> StableHash {
        let status_word = self.status as u64;
        StableHash::of_words(&[self.launch_spec.identity().raw(), status_word])
    }
}
