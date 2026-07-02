//! [`LaunchSpec`] — the canonical description of exactly what to launch, and
//! [`DropInSpec`] — the editor-derived spawn context that can be attached to it.
//!
//! A launch spec is the workspace's contract *to* a future runtime integration:
//! the project id, the version, the entrypoint, and the deterministic fixed step
//! the session should run at. Its identity is a [`StableHash`] over those fields,
//! so the same project/version/config always yields the same launch identity.
//!
//! "Drop In" is: this canonical launch spec **plus** an editor-derived spawn
//! context ([`DropInSpec`]) — where in a level, facing which way, with which
//! entity selected — that a launched session should spawn into. Attaching a drop
//! context is additive: it never changes the launch identity.

use axiom_kernel::StableHash;
use axiom_runtime::RuntimeConfig;

use crate::drop_in_spec::DropInSpec;
use crate::game_project::GameProject;

/// The canonical, launchable description of a game session.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchSpec {
    project_id: String,
    game_version: String,
    entrypoint: String,
    fixed_step_nanos: u64,
    drop_in: Option<DropInSpec>,
}

impl LaunchSpec {
    /// Build a launch spec for a project at the runtime's default deterministic
    /// fixed step ([`RuntimeConfig::DEFAULT_FIXED_STEP_NANOS`]). No drop context.
    #[must_use]
    pub fn for_project(project: &GameProject) -> Self {
        LaunchSpec {
            project_id: project.id().to_string(),
            game_version: project.version().to_string(),
            entrypoint: project.entrypoint().to_string(),
            fixed_step_nanos: RuntimeConfig::DEFAULT_FIXED_STEP_NANOS,
            drop_in: None,
        }
    }

    /// The project this spec launches.
    #[must_use]
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// The version this spec launches.
    #[must_use]
    pub fn game_version(&self) -> &str {
        &self.game_version
    }

    /// The entrypoint this spec launches.
    #[must_use]
    pub fn entrypoint(&self) -> &str {
        &self.entrypoint
    }

    /// The deterministic fixed step (integer nanoseconds) the session runs at.
    #[must_use]
    pub fn fixed_step_nanos(&self) -> u64 {
        self.fixed_step_nanos
    }

    /// The attached editor-derived spawn context, if this is a "Drop In" launch.
    #[must_use]
    pub fn drop_in(&self) -> Option<&DropInSpec> {
        self.drop_in.as_ref()
    }

    /// Attach an editor-derived spawn context, producing a "Drop In" launch. This
    /// is additive: the launch identity ([`LaunchSpec::identity`]) is unchanged,
    /// and every launch-identity field keeps its value.
    #[must_use]
    pub fn with_drop_in(mut self, drop_in: DropInSpec) -> Self {
        self.drop_in = Some(drop_in);
        self
    }

    /// The stable launch identity: a [`StableHash`] over the launch-identity
    /// fields (project id, version, entrypoint, fixed step). The drop context is
    /// deliberately excluded, so the same project/version/config is one identity
    /// whether or not a drop context is attached.
    #[must_use]
    pub fn identity(&self) -> StableHash {
        let mut bytes = Vec::new();
        push_field(&mut bytes, self.project_id.as_bytes());
        push_field(&mut bytes, self.game_version.as_bytes());
        push_field(&mut bytes, self.entrypoint.as_bytes());
        push_field(&mut bytes, &self.fixed_step_nanos.to_le_bytes());
        StableHash::of_bytes(&bytes)
    }
}

/// Append a length-prefixed field to canonical launch-identity bytes, so that
/// `["a", "bc"]` and `["ab", "c"]` never collide into the same digest.
fn push_field(bytes: &mut Vec<u8>, field: &[u8]) {
    bytes.extend_from_slice(&(field.len() as u64).to_le_bytes());
    bytes.extend_from_slice(field);
}
