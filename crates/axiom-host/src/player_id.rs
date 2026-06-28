//! `PlayerId`: an opaque seat index for per-player terminal outcomes.

/// An opaque, stable seat index within a room (SPEC-13 §5).
///
/// `PlayerId` is a primitive `u64` newtype — the seat a per-player
/// [`crate::HostOutcome`] is keyed on in a [`crate::HostOutcomeSet`]. It carries
/// no behaviour: it is the noun the authority hands out, kept primitive so the
/// host boundary names it without pulling in any higher netcode concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerId(u64);

impl PlayerId {
    /// The player at seat `seat`.
    pub const fn new(seat: u64) -> Self {
        PlayerId(seat)
    }

    /// The underlying seat index.
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_round_trips_through_get() {
        assert_eq!(PlayerId::new(3).get(), 3);
    }

    #[test]
    fn equal_seats_compare_equal_and_distinct_seats_differ() {
        assert_eq!(PlayerId::new(1), PlayerId::new(1));
        assert_ne!(PlayerId::new(1), PlayerId::new(2));
    }
}
