//! [`PlayControlsState`] — the typed placeholder state of the Play Controls panel:
//! the current workspace mode plus the optional launch spec and drop-in context
//! the shell would launch with.
//!
//! State-only: this panel holds the *intent* to launch/drop-in as data. It runs,
//! steps, and simulates **nothing** — there are no methods that drive a runtime.
//! It is where Drop-In state references [`LaunchSpec`] and [`DropInSpec`] as data.

use crate::drop_in_spec::DropInSpec;
use crate::launch_spec::LaunchSpec;
use crate::workspace_api::WorkspaceMode;

/// The Play Controls panel state: the workspace mode and the launch / drop-in
/// intent held as data. `Default` is [`WorkspaceMode::Edit`] with nothing staged.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayControlsState {
    mode: WorkspaceMode,
    launch: Option<LaunchSpec>,
    drop_in: Option<DropInSpec>,
}

impl Default for PlayControlsState {
    /// Edit mode with no launch spec and no drop-in context staged.
    fn default() -> Self {
        PlayControlsState {
            mode: WorkspaceMode::Edit,
            launch: None,
            drop_in: None,
        }
    }
}

impl PlayControlsState {
    /// The current workspace mode.
    #[must_use]
    pub fn mode(&self) -> WorkspaceMode {
        self.mode
    }

    /// The staged launch spec, if any — held as data, never run.
    #[must_use]
    pub fn launch(&self) -> Option<&LaunchSpec> {
        self.launch.as_ref()
    }

    /// The staged drop-in context, if any — held as data, never run.
    #[must_use]
    pub fn drop_in(&self) -> Option<&DropInSpec> {
        self.drop_in.as_ref()
    }

    /// Set the workspace mode. No runtime effect — mode is shell state only.
    pub fn set_mode(&mut self, mode: WorkspaceMode) {
        self.mode = mode;
    }

    /// Return an updated state with a launch spec staged as data.
    #[must_use]
    pub fn with_launch(mut self, launch: LaunchSpec) -> Self {
        self.launch = Some(launch);
        self
    }

    /// Return an updated state with a drop-in context staged as data.
    #[must_use]
    pub fn with_drop_in(mut self, drop_in: DropInSpec) -> Self {
        self.drop_in = Some(drop_in);
        self
    }
}
