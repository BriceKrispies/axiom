//! [`ArtifactKind`] — which of a frame's opaque artifacts first diverged.
//!
//! A pure, fieldless value type used by [`crate::determinism_report`] to point at
//! the artifact whose bytes first differed between an original and a replayed
//! timeline. The kinds mirror the four opaque byte payloads on a capture plus the
//! combined `final_hash`. The module never interprets the bytes — this only
//! *names* which payload diverged.

/// Which artifact of a frame is being referred to (e.g. the first to diverge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    /// The opaque input artifact bytes.
    Input,
    /// The opaque runtime-step artifact bytes.
    Runtime,
    /// The opaque state/snapshot artifact bytes.
    State,
    /// The opaque render-command artifact bytes.
    Render,
    /// The combined per-frame `final_hash` (identity + all artifact hashes).
    Final,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_distinct_and_copy() {
        let all = [
            ArtifactKind::Input,
            ArtifactKind::Runtime,
            ArtifactKind::State,
            ArtifactKind::Render,
            ArtifactKind::Final,
        ];
        // Each kind equals only itself.
        all.iter().enumerate().for_each(|(i, a)| {
            all.iter().enumerate().for_each(|(j, b)| {
                assert_eq!(a == b, i == j);
            });
        });
        let copied = ArtifactKind::State;
        assert_eq!(copied, ArtifactKind::State);
        assert!(format!("{copied:?}").contains("State"));
    }
}
