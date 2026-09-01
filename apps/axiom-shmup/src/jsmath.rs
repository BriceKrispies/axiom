//! JavaScript builtin semantics — **the engine's numerics**, named the way the
//! source names them.
//!
//! The transcriptions used to live here. They are now `axiom_math`'s, promoted
//! under the Branchless and Coverage Laws:
//!
//! | this module  | `axiom_math`                        |
//! |--------------|-------------------------------------|
//! | [`hypot`]    | [`axiom_math::hypot`]               |
//! | [`sign`]     | [`axiom_math::signum_with_zero`]    |
//! | [`round`]    | [`axiom_math::round_ties_up`]       |
//! | [`or`]       | [`axiom_math::nonzero_or`]          |
//!
//! ## Why they were engine primitives all along
//!
//! Nothing in the list is really about JavaScript. A max-scaled, Kahan-
//! compensated norm is simply a *more accurate* norm than `sqrt(x*x+y*y+z*z)`
//! — it disagrees with the naive form on 37.5% of metre-scale triples, and it
//! is the naive form that is wrong. A three-valued sign that returns zero for
//! zero is the one you want wherever a sign multiplies a magnitude, because
//! `f64::signum` turns a body at rest into a full-magnitude jump. Ties-toward-
//! `+∞` is a real rounding mode, and which mode you quantise a lattice with
//! decides *which cell* a boundary value lands in. A falsy-replace is a
//! degenerate-divisor guard, and the thing it is not — a clamp — is a mistake
//! this port has already made once in `rigidbody`.
//!
//! JavaScript is where the port *met* these functions, not what they are.
//!
//! ## What this file still earns
//!
//! The names. Every call site here was transcribed from a source line that says
//! `Math.hypot` / `Math.sign` / `Math.round` / `|| 1`, and `hypot3(dx, dy, dz)`
//! diffs against `Math.hypot(dx, dy, dz)` by eye where
//! `axiom_math::hypot3(...)` would need the reader to know they are the same
//! function. The mapping table above is the one place that has to be checked,
//! rather than fifty call sites.
//!
//! ## The history worth keeping
//!
//! Before a single shared module existed, this crate held **six** independent
//! `hypot3` implementations and **nine** independent three-valued `sign`s. Three
//! of the six transcribed the compensated algorithm correctly, at three times
//! the cost; one used the uncompensated max-scaled form; one used the plain root
//! with a comment reasoning the difference was "within a couple of ULP" and
//! could not matter; and the sixth used the plain root too, **citing that
//! comment as its justification**.
//!
//! That last step is the one to remember: a wrong implementation propagated by
//! citation, because the reasoning that excused it read as authority. A
//! duplicated primitive does not merely cost duplication — it lets two copies
//! disagree and hands each one a plausible local argument for why it is fine.
//! The measurement settled it: the plain root disagreed with V8 on 1,538 of
//! 4,096 sampled triples. One ULP is not nothing when `rigidbody` renormalises
//! a quaternion every step and feeds the result through the inertia tensor into
//! the contact solver.
//!
//! The same shape recurred once more inside this promotion. `world/noise.rs`
//! carried its own `round_half_up` as `(v + 0.5).floor()`, documented as
//! reproducing `Math.round` "exactly for every finite `v`" — which this
//! module's own [`round`] documentation had already shown to be false at
//! `0.49999999999999994`. Two `Math.round`s in one crate, one of them correct,
//! and the wrong one was the one first promoted into the engine. Consolidating
//! on [`axiom_math::round_ties_up`] fixed it; the noise goldens did not move,
//! because the pathological input never arose in them — which is exactly how a
//! latent defect survives a golden suite.

pub use axiom_math::{hypot, hypot2, hypot3, hypot4};

/// `Math.sign(x)` — three-valued, unlike [`f64::signum`].
pub fn sign(x: f64) -> f64 {
    axiom_math::signum_with_zero(x)
}

/// `Math.round(x)` — ties break toward `+Infinity`, not away from zero.
pub fn round(x: f64) -> f64 {
    axiom_math::round_ties_up(x)
}

/// JavaScript's `expr || fallback` — a falsy-replace, **not** a clamp.
pub fn or(v: f64, fallback: f64) -> f64 {
    axiom_math::nonzero_or(v, fallback)
}

/// JavaScript's `expr || 1`.
pub fn or_one(v: f64) -> f64 {
    axiom_math::nonzero_or_one(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The binding, not the algorithms — those are pinned in `axiom-math`'s own
    /// tests. What can still go wrong here is a name wired to the wrong
    /// function, so each one is checked at the input that distinguishes it from
    /// its plausible neighbour.
    #[test]
    fn every_name_is_wired_to_the_function_the_source_means() {
        // `Math.hypot`, not the plain root: compensation shows from three
        // components on.
        let (x, y, z) = (
            8.907_641_209_661_96_f64,
            -9.805_145_198_479_295,
            9.456_697_767_600_417,
        );
        assert_ne!(
            hypot3(x, y, z).to_bits(),
            (x * x + y * y + z * z).sqrt().to_bits()
        );

        // `Math.sign`, not `f64::signum`.
        assert_eq!(sign(0.0), 0.0);
        assert!(sign(-0.0).is_sign_negative());
        assert_eq!(0.0_f64.signum(), 1.0, "the divergence this avoids");

        // `Math.round`, not `f64::round` and not `(x + 0.5).floor()`.
        assert_eq!(round(-2.5), -2.0);
        assert_eq!((-2.5_f64).round(), -3.0, "the divergence this avoids");
        assert_eq!(round(0.499_999_999_999_999_94), 0.0);

        // `|| fallback`, not `max(fallback)`.
        assert_eq!(or(5e-5, 1e-4), 5e-5);
        assert_eq!(5e-5_f64.max(1e-4), 1e-4, "the clamp this is not");
        assert_eq!(or_one(f64::NAN), 1.0);
    }
}
