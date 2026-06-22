//! A deterministic, integer-backed quantity model for simulation amounts.

/// The unit a [`Quantity`] is measured in. Operations are only valid between
/// quantities of the same unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QuantityUnit {
    /// A discrete count of things.
    Count,
    /// A mass amount.
    Mass,
    /// A volume amount.
    Volume,
    /// A dose amount.
    Dose,
    /// An arbitrary, domain-defined simulation unit.
    Arbitrary,
}

/// A non-negative, integer-backed amount in a fixed [`QuantityUnit`].
///
/// Quantities use `i64` counts (no floating point), so all arithmetic is exact
/// and deterministic. Construction rejects negative amounts; arithmetic is only
/// defined between equal units and fails cleanly (returning `None`) on unit
/// mismatch, overflow, or a result that would go negative. Ordering derives over
/// `(unit, amount)` for stable storage; semantic comparison uses
/// [`Self::compare`], which is `None` across incompatible units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Quantity {
    unit: QuantityUnit,
    amount: i64,
}

impl Quantity {
    /// Construct a quantity, rejecting a negative amount.
    pub fn new(unit: QuantityUnit, amount: i64) -> Option<Self> {
        (amount >= 0).then_some(Quantity { unit, amount })
    }

    /// A zero quantity in `unit`.
    pub const fn zero(unit: QuantityUnit) -> Self {
        Quantity { unit, amount: 0 }
    }

    /// The unit.
    pub const fn unit(self) -> QuantityUnit {
        self.unit
    }

    /// The amount.
    pub const fn amount(self) -> i64 {
        self.amount
    }

    /// Whether the amount is zero.
    pub const fn is_zero(self) -> bool {
        self.amount == 0
    }

    /// Add a compatible quantity. `None` if units differ or the sum overflows.
    pub fn add(self, other: Quantity) -> Option<Quantity> {
        (self.unit == other.unit)
            .then(|| self.amount.checked_add(other.amount))
            .flatten()
            .map(|amount| Quantity {
                unit: self.unit,
                amount,
            })
    }

    /// Subtract a compatible quantity. `None` if units differ, or the result
    /// would be negative (insufficient amount).
    pub fn sub(self, other: Quantity) -> Option<Quantity> {
        (self.unit == other.unit)
            .then(|| self.amount.checked_sub(other.amount))
            .flatten()
            .filter(|remaining| *remaining >= 0)
            .map(|amount| Quantity {
                unit: self.unit,
                amount,
            })
    }

    /// Compare with a compatible quantity. `None` across incompatible units.
    pub fn compare(self, other: Quantity) -> Option<std::cmp::Ordering> {
        (self.unit == other.unit).then(|| self.amount.cmp(&other.amount))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn construction_rejects_negative_amounts() {
        assert!(Quantity::new(QuantityUnit::Mass, -1).is_none());
        let q = Quantity::new(QuantityUnit::Mass, 5).unwrap();
        assert_eq!(q.unit(), QuantityUnit::Mass);
        assert_eq!(q.amount(), 5);
        assert!(!q.is_zero());
        assert!(Quantity::zero(QuantityUnit::Count).is_zero());
    }

    #[test]
    fn add_and_subtract_compatible_units() {
        let a = Quantity::new(QuantityUnit::Volume, 10).unwrap();
        let b = Quantity::new(QuantityUnit::Volume, 3).unwrap();
        assert_eq!(a.add(b).unwrap().amount(), 13);
        assert_eq!(a.sub(b).unwrap().amount(), 7);
    }

    #[test]
    fn incompatible_units_reject_operations() {
        let mass = Quantity::new(QuantityUnit::Mass, 5).unwrap();
        let dose = Quantity::new(QuantityUnit::Dose, 5).unwrap();
        assert!(mass.add(dose).is_none());
        assert!(mass.sub(dose).is_none());
        assert!(mass.compare(dose).is_none());
    }

    #[test]
    fn subtraction_fails_on_insufficient_amount() {
        let a = Quantity::new(QuantityUnit::Count, 3).unwrap();
        let b = Quantity::new(QuantityUnit::Count, 5).unwrap();
        assert!(a.sub(b).is_none(), "3 - 5 is insufficient");
        assert_eq!(a.sub(a).unwrap().amount(), 0);
    }

    #[test]
    fn add_overflow_fails_cleanly() {
        let a = Quantity::new(QuantityUnit::Arbitrary, i64::MAX).unwrap();
        let b = Quantity::new(QuantityUnit::Arbitrary, 1).unwrap();
        assert!(a.add(b).is_none());
    }

    #[test]
    fn comparison_is_deterministic_within_a_unit() {
        let a = Quantity::new(QuantityUnit::Dose, 2).unwrap();
        let b = Quantity::new(QuantityUnit::Dose, 7).unwrap();
        assert_eq!(a.compare(b), Some(Ordering::Less));
        assert_eq!(b.compare(a), Some(Ordering::Greater));
        assert_eq!(a.compare(a), Some(Ordering::Equal));
    }
}
