//! A tally of inbound frames the session refused, by reason.

/// How many ingested frames a session dropped, grouped by why.
///
/// Runtime telemetry for observing a live session under load or attack: an
/// honest peer silently drops forged, unknown-peer, and out-of-window frames
/// (they never touch confirmed state), and this counts them so a harness or
/// monitor can see, e.g., how many forgeries a victim shrugged off. It is not
/// re-exported from the crate root — read it through
/// [`crate::NetcodeApi::rejections`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rejections {
    /// Frames claiming a peer that is not in the roster.
    pub unknown_peer: u64,
    /// Frames whose signature did not verify against the claimed author's key.
    pub bad_signature: u64,
    /// Well-formed, validly-signed frames whose tick fell outside the admission
    /// window (a past tick, or beyond the look-ahead horizon).
    pub out_of_window: u64,
}

impl Rejections {
    /// The total number of dropped frames across all reasons.
    pub fn total(&self) -> u64 {
        self.unknown_peer + self.bad_signature + self.out_of_window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_zero_and_total_sums() {
        let r = Rejections::default();
        assert_eq!(r, Rejections::default());
        assert_eq!(r.total(), 0);
        let r = Rejections {
            unknown_peer: 2,
            bad_signature: 3,
            out_of_window: 5,
        };
        assert_eq!(r.total(), 10);
    }
}
