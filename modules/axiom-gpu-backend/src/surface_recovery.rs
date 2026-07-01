//! The deterministic decision for recovering a lost/outdated GPU surface.
//!
//! A mobile browser aggressively drops the WebGPU/WebGL drawing context when the
//! tab is backgrounded; on the next frame the surface reports an error instead of
//! a texture. This module is the pure policy half of handling that: it maps the
//! kind of surface failure to the action the live binding should take, with no
//! `wgpu` or browser types in sight, so it is native-compiled and fully covered.
//! The wasm32 binding translates `wgpu::SurfaceError` into a [`SurfaceStatus`]
//! and carries out the [`RecoveryAction`] this returns.

/// Why a swap-chain texture acquisition **failed** — the engine's own mirror of
/// the `wgpu::SurfaceError` cases, so the recovery decision can be reasoned about
/// and tested without a GPU. (Success is not a status here: the live binding
/// presents an acquired frame directly and only consults this on an error.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SurfaceStatus {
    /// Acquisition timed out — transient; skip this frame and try the next.
    Timeout,
    /// The surface is outdated (e.g. after a resize); reconfigure and retry.
    Outdated,
    /// The drawing context was lost (a backgrounded mobile tab); reconfigure to
    /// re-acquire it.
    Lost,
    /// The device is out of memory; a reconfigure will not help — rebuild.
    OutOfMemory,
    /// Any other acquisition failure; reconfigure is the safe default attempt.
    Other,
}

/// What the live binding should do this frame in response to a failed
/// acquisition's [`SurfaceStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RecoveryAction {
    /// Drop this frame without reconfiguring (a transient hiccup).
    SkipFrame,
    /// Reconfigure the surface with its stored config, then re-acquire.
    Reconfigure,
    /// Tear down and rebuild the whole binding (device/surface/renderer).
    Reinitialize,
}

impl SurfaceStatus {
    /// The recovery action for this failure. Branchless: a fieldless enum's
    /// discriminant indexes the action table, so each status maps to exactly one
    /// action with no control flow.
    pub(crate) const fn recovery_action(self) -> RecoveryAction {
        [
            RecoveryAction::SkipFrame,    // Timeout
            RecoveryAction::Reconfigure,  // Outdated
            RecoveryAction::Reconfigure,  // Lost
            RecoveryAction::Reinitialize, // OutOfMemory
            RecoveryAction::Reconfigure,  // Other
        ][self as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_skips_without_reconfiguring() {
        assert_eq!(
            SurfaceStatus::Timeout.recovery_action(),
            RecoveryAction::SkipFrame
        );
    }

    #[test]
    fn outdated_and_lost_and_other_reconfigure() {
        assert_eq!(
            SurfaceStatus::Outdated.recovery_action(),
            RecoveryAction::Reconfigure
        );
        assert_eq!(
            SurfaceStatus::Lost.recovery_action(),
            RecoveryAction::Reconfigure
        );
        assert_eq!(
            SurfaceStatus::Other.recovery_action(),
            RecoveryAction::Reconfigure
        );
    }

    #[test]
    fn out_of_memory_demands_a_full_rebuild() {
        assert_eq!(
            SurfaceStatus::OutOfMemory.recovery_action(),
            RecoveryAction::Reinitialize
        );
    }

    #[test]
    fn statuses_and_actions_are_distinct_value_types() {
        assert_ne!(SurfaceStatus::Timeout, SurfaceStatus::Lost);
        assert_ne!(RecoveryAction::SkipFrame, RecoveryAction::Reconfigure);
        let s = SurfaceStatus::Outdated;
        assert_eq!(s, s);
        let a = RecoveryAction::SkipFrame;
        assert_eq!(a, a);
    }
}
