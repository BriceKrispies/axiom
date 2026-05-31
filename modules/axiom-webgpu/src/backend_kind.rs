//! Which backend the `WebGpuApi` is operating in.

/// Which backend mode `WebGpuApi` is operating in.
///
/// - [`BackendKind::Recording`] captures every submission into a
///   deterministic [`crate::GpuSubmissionReport`] and never touches a GPU.
///   It is the proof backend the headless vertical slice relies on.
/// - [`BackendKind::Live`] is the structural seam for real presentation. A
///   live backend consumes the deterministic host presentation boundary
///   (`axiom_host::HostPresentationRequest` and friends) and accepts the
///   *same* [`crate::GpuSubmission`] shape, but does not perform real GPU
///   work in this pass — see `ARCHITECTURE.md`.
///
/// This is the coarse two-way kind. The richer live state (unbound vs
/// presentation-requested) lives in the module-internal
/// `WebGpuBackendState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// Capture submissions as a deterministic record. No GPU.
    Recording,
    /// Structurally model live presentation from host-owned data. No real
    /// GPU work happens in this pass.
    Live,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_kinds_are_distinct() {
        assert_ne!(BackendKind::Recording, BackendKind::Live);
    }

    #[test]
    fn kinds_are_copy_and_equal() {
        let k = BackendKind::Live;
        let j = k;
        assert_eq!(k, j);
    }
}
