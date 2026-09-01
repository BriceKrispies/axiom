//! A three-valued sign that returns zero for zero.

/// The sign of `x` as `-1.0`, `+0.0`/`-0.0`, or `+1.0` — **three-valued**,
/// unlike [`f64::signum`].
///
/// [`f64::signum`] is two-valued on the reals: it returns `+1.0` for `+0.0` and
/// `-1.0` for `-0.0`, because it reports the sign *bit* rather than the sign of
/// the value. That is the right answer to a different question, and it is a
/// trap wherever a sign is multiplied straight into a magnitude:
///
/// ```text
/// velocity += direction.signum() * step;          // a body at rest jumps
/// velocity += signum_with_zero(direction) * step; // a body at rest stays
/// ```
///
/// A zero input should contribute nothing, and with [`f64::signum`] it
/// contributes a full step in whichever direction the sign bit happened to
/// hold.
///
/// The zero is returned *as it came in*, sign intact, so `-0.0` stays `-0.0`
/// and anything downstream that tests the sign bit sees what it was given. NaN
/// likewise passes through rather than becoming a direction.
pub fn signum_with_zero(x: f64) -> f64 {
    // Zero and NaN are the two inputs with no direction to report.
    let directionless = x.is_nan() | (x == 0.0);
    let direction = [-1.0_f64, 1.0][usize::from(x > 0.0)];
    [direction, x][usize::from(directionless)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_direction_of_a_nonzero_value() {
        assert_eq!(signum_with_zero(3.5), 1.0);
        assert_eq!(signum_with_zero(-3.5), -1.0);
        assert_eq!(signum_with_zero(f64::MIN_POSITIVE), 1.0);
        assert_eq!(signum_with_zero(f64::INFINITY), 1.0);
        assert_eq!(signum_with_zero(f64::NEG_INFINITY), -1.0);
    }

    #[test]
    fn zero_has_no_direction_and_keeps_its_sign() {
        assert_eq!(signum_with_zero(0.0), 0.0);
        assert!(signum_with_zero(0.0).is_sign_positive());
        assert_eq!(signum_with_zero(-0.0), 0.0);
        assert!(signum_with_zero(-0.0).is_sign_negative());
    }

    #[test]
    fn nan_passes_through_rather_than_becoming_a_direction() {
        assert!(signum_with_zero(f64::NAN).is_nan());
    }

    /// The divergence this function exists to prevent, stated directly.
    #[test]
    fn differs_from_f64_signum_at_zero() {
        assert_eq!(0.0_f64.signum(), 1.0);
        assert_eq!((-0.0_f64).signum(), -1.0);
        assert_eq!(signum_with_zero(0.0), 0.0);
        assert_eq!(signum_with_zero(-0.0), 0.0);
    }

    /// The failure mode in the doc comment, as a test: a value at rest must
    /// stay at rest.
    #[test]
    fn a_zero_direction_contributes_nothing_to_a_step() {
        let step = 5.0;
        assert_eq!(signum_with_zero(0.0) * step, 0.0);
        assert_eq!(0.0_f64.signum() * step, 5.0, "the trap this avoids");
    }
}
