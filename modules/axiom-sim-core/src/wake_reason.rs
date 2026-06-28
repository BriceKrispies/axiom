//! Why a scheduler-managed process was placed on the wake schedule.
//!
//! `WakeReason` is the payload sim-core rides on the kernel's generic
//! [`axiom_kernel::TickSchedule`]: the schedule owns the deterministic
//! `(tick, process)` ordering and bookkeeping, and this enum is the
//! domain-specific reason carried alongside each pending wake.

/// Why a process was placed on the wake schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WakeReason {
    /// Scheduled explicitly at registration/scheduling time.
    Scheduled,
    /// Re-scheduled to a new tick.
    Rescheduled,
    /// Woken because a dependency it subscribed to went dirty.
    DirtyDependency,
    /// Woken by an explicit external request.
    Explicit,
    /// An unclassified wake.
    Generic,
}

const WAKE_REASONS: [WakeReason; 5] = [
    WakeReason::Scheduled,
    WakeReason::Rescheduled,
    WakeReason::DirtyDependency,
    WakeReason::Explicit,
    WakeReason::Generic,
];

impl WakeReason {
    /// Construct from a code, `None` if out of range.
    pub fn from_code(code: u8) -> Option<WakeReason> {
        WAKE_REASONS.get(code as usize).copied()
    }

    /// The reason's deterministic code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_reason_codes_round_trip() {
        assert_eq!(WakeReason::from_code(0), Some(WakeReason::Scheduled));
        assert_eq!(WakeReason::from_code(4), Some(WakeReason::Generic));
        assert_eq!(WakeReason::from_code(5), None);
        assert_eq!(WakeReason::DirtyDependency.code(), 2);
    }
}
