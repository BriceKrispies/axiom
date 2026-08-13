//! The authored-literal quantity constructors.
//!
//! Every mesh operator in `axiom-mesh-ops` speaks unit-safe kernel quantities
//! rather than naked floats, and this app authors hundreds of them from literal
//! constants and from arithmetic over literal constants. All of those values are
//! finite by construction, so the total `finite_or_zero` constructors are the
//! honest path: a fallible `Meters::new` here would leave an error arm no test
//! could ever provoke.
//!
//! Nothing else lives in this file. It is a vocabulary, not a drawer.

use axiom_kernel::{Meters, Radians, Ratio};

/// A length from an authored literal (or arithmetic over authored literals).
pub fn meters(value: f32) -> Meters {
    Meters::finite_or_zero(value)
}

/// An angle from an authored literal.
pub fn radians(value: f32) -> Radians {
    Radians::finite_or_zero(value)
}

/// A dimensionless fraction/scale from an authored literal.
pub fn ratio(value: f32) -> Ratio {
    Ratio::finite_or_zero(value)
}
