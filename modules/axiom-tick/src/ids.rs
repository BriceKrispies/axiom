//! Deterministic identity newtypes the timers + state-machine facade hands out.
//!
//! A [`TimerId`] names a scheduled timer; a [`StateMachineId`] names a state
//! machine. Both are `u64`-backed, minted ascending by the owning collection
//! (never random, never wall-clock), and totally ordered — the ascending order
//! is exactly the deterministic tie-break for two timers due on the same tick
//! and for ordering state events across machines.

/// Define a `u64`-backed deterministic id newtype.
macro_rules! tick_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            /// Construct this id from a raw value.
            pub const fn from_raw(raw: u64) -> Self {
                $name(raw)
            }

            /// The raw value backing this id.
            pub const fn raw(self) -> u64 {
                self.0
            }
        }
    };
}

tick_id!(TimerId, "A deterministic identity for a scheduled timer.");
tick_id!(
    StateMachineId,
    "A deterministic identity for a state machine."
);

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercise the full generated surface (raw round-trip, ordering, equality,
    /// hashing, clone, debug) for one id type.
    macro_rules! id_behaviour {
        ($name:ident, $test:ident) => {
            #[test]
            fn $test() {
                use std::collections::HashSet;
                assert_eq!($name::from_raw(7).raw(), 7);
                assert!($name::from_raw(1) < $name::from_raw(2));
                assert_eq!($name::from_raw(5), $name::from_raw(5));
                assert_ne!($name::from_raw(5), $name::from_raw(6));
                let mut set = HashSet::new();
                set.insert($name::from_raw(3));
                set.insert($name::from_raw(3));
                set.insert($name::from_raw(4));
                assert_eq!(set.len(), 2);
                let copy = $name::from_raw(9);
                assert_eq!(copy.clone(), copy);
                assert_eq!(format!("{copy:?}"), format!("{}(9)", stringify!($name)));
            }
        };
    }

    id_behaviour!(TimerId, timer_id_behaviour);
    id_behaviour!(StateMachineId, state_machine_id_behaviour);
}
