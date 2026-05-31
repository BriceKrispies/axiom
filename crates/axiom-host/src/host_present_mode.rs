//! Abstract presentation pacing mode for a host surface.

/// How a future live surface should pace presentation.
///
/// This is an **abstract** host-boundary enum. It does not name a WebGPU,
/// WebGL, or OS present mode; a future browser/native adapter maps these
/// variants onto whatever the real backend exposes. The host layer only
/// needs to *describe* the engine's intent deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostPresentMode {
    /// Block on vertical sync (the energy-conscious default).
    Fifo,
    /// Present as soon as a frame is ready (may tear).
    Immediate,
    /// Present the most recent frame, dropping older ones.
    Mailbox,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_distinct() {
        assert_ne!(HostPresentMode::Fifo, HostPresentMode::Immediate);
        assert_ne!(HostPresentMode::Immediate, HostPresentMode::Mailbox);
        assert_ne!(HostPresentMode::Fifo, HostPresentMode::Mailbox);
    }

    #[test]
    fn variants_are_copy_and_equal() {
        let m = HostPresentMode::Fifo;
        let n = m;
        assert_eq!(m, n);
    }
}
