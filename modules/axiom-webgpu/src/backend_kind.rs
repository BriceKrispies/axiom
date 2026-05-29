//! Which backend the `WebGpuApi` is operating in today.

/// Which backend mode `WebGpuApi` is operating in.
///
/// The vertical slice ships only the `Recording` backend: every
/// `submit()` call captures the submission into a deterministic
/// [`crate::GpuSubmissionReport`] but does not touch a GPU. A future
/// `Live` backend will perform real wgpu/web-sys calls once the host
/// layer exposes a surface (see `ARCHITECTURE.md` for the blocker).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// Capture submissions as a deterministic record. No GPU.
    Recording,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_is_the_only_variant_today() {
        let k = BackendKind::Recording;
        assert_eq!(k, BackendKind::Recording);
    }
}
