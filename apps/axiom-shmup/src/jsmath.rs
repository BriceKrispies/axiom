//! JavaScript builtin semantics, transcribed from V8 once.
//!
//! Not ported from any one Claude-of-Duty file. This module exists because
//! **the source's arithmetic is JavaScript arithmetic**, and three of its
//! builtins do not mean in Rust what they mean in JS:
//!
//! | JavaScript      | the naive Rust        | why it differs                    |
//! |-----------------|-----------------------|-----------------------------------|
//! | `Math.hypot`    | `(x*x+y*y+z*z).sqrt()`| V8 max-scales and Kahan-compensates |
//! | `Math.sign`     | `f64::signum`         | `signum(±0.0)` is `±1.0`, not `±0.0` |
//! | `expr \|\| 1`   | `if v == 0.0`         | JS falsiness catches `NaN` too    |
//! | `Math.round`    | `f64::round`          | ties go to `+Infinity`, not away from zero |
//!
//! `Math.hypot` and `Math.sign` are both named in the port recipe's trap list
//! ("`sign` is not `signum`", "`Math.hypot` is not `sqrt(x*x + y*y + z*z)`").
//! `Math.round` is not in that list and should be: it was independently
//! rediscovered by **six** separate slices (`ai/geo`, `audio/foley`,
//! `materials/masks`, `materials/system`, `physics/ragdoll`, `sky/volumetrics`),
//! each writing its own `js_round` and its own explanatory comment.
//!
//! ## Why this is a crate-level module and not a helper in each subsystem
//!
//! It was a helper in each subsystem, and that is exactly how the port got a
//! wrong answer into shipped code.
//!
//! Before this module existed the crate held **six** independent `hypot3`
//! implementations and **nine** independent three-valued `sign`s. The six
//! `hypot3`s used three different algorithms:
//!
//! - `ai/nav.rs`, `ai/parts.rs`, `physics/debug.rs` each independently
//!   transcribed V8's real algorithm — correct, three times over, at three
//!   times the cost.
//! - `physics/rigidbody.rs` used the uncompensated max-scaled form.
//! - `audio/spatial.rs` used the plain root, with a comment reasoning that the
//!   difference was "within a couple of ULP" and could not matter.
//! - `ai/geo.rs` then used the plain root too, and cited `audio::spatial`'s
//!   comment as its justification.
//!
//! That last step is the one worth naming: a wrong implementation propagated by
//! citation, because the reasoning that excused it read as authority. A
//! duplicated primitive does not merely cost duplication — it lets two copies
//! disagree and gives each one a plausible local argument for why it is fine.
//!
//! The claim was also measurably false. `tests/jsmath/capture.mjs` samples
//! 4,096 metre-scale triples and compares each candidate to V8 bit for bit:
//! the plain root disagrees on **1,538** of them (37.5%) and the uncompensated
//! max-scaled form on **191** (4.7%). Both by one ULP — and one ULP is not
//! nothing here, because `rigidbody.js` renormalises its quaternion every step
//! and feeds the result through the world inertia tensor into the contact
//! solver, so the error compounds from first contact onward.
//!
//! So the primitive lives once, is transcribed from V8 once, and is pinned
//! against V8 once, in `tests/jsmath_port.rs`. That test is the definition of
//! correct for this module; the doc comments here only explain it.
//!
//! ## The transcription
//!
//! [`hypot`] follows V8's `MathHypot` statement for statement — the infinity
//! short-circuit before the NaN fallthrough, the `max == 0 -> max = 1`
//! substitution rather than an early return, and Kahan compensation carried
//! across the summands in argument order. With two arguments the compensation
//! term is only ever produced and never consumed, which is why a two-argument
//! `hypot` happens to agree with the uncompensated form; with three or more it
//! is consumed and they diverge. That is precisely why the three-argument
//! callers were the ones that went wrong.

/// `Math.hypot(...args)` — V8's `MathHypot`, transcribed statement for
/// statement.
///
/// Rust's `f64::hypot` is a *different* algorithm (a correctly-rounded
/// two-argument form) and does not agree with this in the last bits, so it is
/// not an alternative even for two arguments.
///
/// Edge behaviour, all of it load-bearing and all of it pinned:
///
/// - any argument infinite -> `Infinity`, **checked before NaN**, so
///   `Math.hypot(NaN, Infinity)` is `Infinity` and not `NaN`;
/// - otherwise any argument NaN -> `NaN`, which falls out of the sum rather
///   than being special-cased (`n > max` is false for NaN, so NaN never
///   becomes the scale);
/// - all arguments zero -> `0.0`, via the `max = 1` substitution.
pub fn hypot(args: &[f64]) -> f64 {
    let mut max = 0.0_f64;
    for &v in args {
        let n = v.abs();
        if n == f64::INFINITY {
            return f64::INFINITY;
        }
        // `NaN > max` is false, so a NaN argument never becomes the scale —
        // it reaches the sum instead and poisons it, which is what V8 does.
        if n > max {
            max = n;
        }
    }
    // V8 substitutes 1 rather than returning early, so the all-zero case still
    // runs the sum (and still yields +0.0).
    if max == 0.0 {
        max = 1.0;
    }
    let mut sum = 0.0_f64;
    let mut compensation = 0.0_f64;
    for &v in args {
        let n = v.abs() / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// `Math.hypot(x, y)`.
pub fn hypot2(x: f64, y: f64) -> f64 {
    hypot(&[x, y])
}

/// `Math.hypot(x, y, z)`.
pub fn hypot3(x: f64, y: f64, z: f64) -> f64 {
    hypot(&[x, y, z])
}

/// `Math.hypot(x, y, z, w)` — the quaternion normalisation in
/// `physics/rigidbody.js`.
pub fn hypot4(x: f64, y: f64, z: f64, w: f64) -> f64 {
    hypot(&[x, y, z, w])
}

/// `Math.sign(x)` — **three-valued**, unlike [`f64::signum`].
///
/// `f64::signum` returns `1.0` for `+0.0` and `-1.0` for `-0.0`; `Math.sign`
/// returns the zero back with its sign intact, and `NaN` for `NaN`. Wherever
/// the source multiplies a sign straight into a magnitude, `signum` turns a
/// zero that should contribute nothing into a full-magnitude jump.
pub fn sign(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        // Returns `x` itself, not a literal `0.0`: `Math.sign(-0)` is `-0`,
        // and `Math.sign(NaN)` is `NaN`.
        return x;
    }
    if x > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// `Math.round(x)` — ties break toward **`+Infinity`**, not away from zero.
///
/// [`f64::round`] breaks ties away from zero, so the two disagree on every
/// negative half-integer: `Math.round(-2.5)` is `-2`, `(-2.5f64).round()` is
/// `-3`. `Math.round(-0.5)` is `-0`, and `Math.round(-0.2)` is `-0` too — the
/// sign survives, which matters wherever the result is divided into or
/// compared against zero.
///
/// This is not a rounding nicety. In `physics/ragdoll.js` the rounded value
/// decides whether two bone endpoints merge into a single particle, so the tie
/// rule changes the *topology* of the doll; in `materials/masks.rs` it
/// quantises a position to a 1/8192 grid that a mask is keyed on.
///
/// ## `floor(x + 0.5)` is *not* the rule, and the first draft of this function
/// was wrong because of it
///
/// The obvious transcription is `(x + 0.5).floor()`. It is wrong for
/// `x = 0.49999999999999994`, the largest double below `0.5`: adding `0.5`
/// rounds up to exactly `1.0`, so `floor` yields `1` where `Math.round` yields
/// `+0`. ECMA-262 does not define `Math.round` as `floor(x + 0.5)` — it states
/// *"if x is less than 0.5 but greater than 0, return +0"* before it mentions
/// flooring at all, precisely to head off that double-rounding.
///
/// The golden caught this on its first run, which is a fair advertisement for
/// the method: the bug was in a function written specifically to fix a rounding
/// trap, by someone who had just finished reading the trap list.
pub fn round(x: f64) -> f64 {
    // NaN, ±Infinity and ±0 all come back unchanged, sign intact.
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    // ECMA-262's two explicit sub-0.5 clauses, stated before any flooring.
    // These are what make `Math.round(0.49999999999999994)` be `+0` and
    // `Math.round(-0.5)` be `-0`.
    if x > 0.0 && x < 0.5 {
        return 0.0;
    }
    if x < 0.0 && x >= -0.5 {
        return -0.0;
    }
    let r = (x + 0.5).floor();
    // Same double-rounding, at larger magnitudes: if adding 0.5 carried across
    // an integer boundary it should not have, back it out. An exact tie gives
    // `r - x == 0.5` and is deliberately left alone — that tie rounding toward
    // `+Infinity` is the whole behaviour this function exists to reproduce.
    if r - x > 0.5 {
        return r - 1.0;
    }
    r
}

/// JavaScript's `expr || 1`.
///
/// Both `±0.0` **and** `NaN` are falsy in JavaScript, so both fall through to
/// `1.0`. A Rust `if v == 0.0 { 1.0 } else { v }` propagates the NaN instead,
/// and `f64::max(1.0)` does something different again. The source leans on
/// this to keep a degenerate direction from going NaN — it collapses to the
/// unit divisor and the geometry stays finite.
pub fn or_one(v: f64) -> f64 {
    or(v, 1.0)
}

/// JavaScript's `expr || fallback`, for a fallback that is not `1`.
///
/// **This is a falsy-replace, not a clamp, and the two are different
/// functions.** `Math.sqrt(d2) || 1e-4` keeps a distance of `5e-5` — it is
/// truthy — where `d.max(1e-4)` pushes it up to `1e-4`. They agree only at
/// exactly zero and above the fallback.
///
/// `physics/rigidbody.js:269` is the site that made this worth naming: the
/// port had written `.max(1e-4)`, which silently strengthened every radial
/// impulse applied to a body within 0.1 mm of the blast centre.
pub fn or(v: f64, fallback: f64) -> f64 {
    if v == 0.0 || v.is_nan() {
        fallback
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The exhaustive pin against V8 lives in `tests/jsmath_port.rs`, which
    // reads a golden captured from Node. These cover only the properties a
    // reader needs stated inline to trust the code above.

    #[test]
    fn hypot_infinity_short_circuits_before_nan() {
        // Argument order must not matter, and NaN must not win.
        assert_eq!(hypot2(f64::NAN, f64::INFINITY), f64::INFINITY);
        assert_eq!(hypot2(f64::INFINITY, f64::NAN), f64::INFINITY);
        assert_eq!(hypot3(f64::NAN, 1.0, f64::NEG_INFINITY), f64::INFINITY);
    }

    #[test]
    fn hypot_nan_without_infinity_is_nan() {
        assert!(hypot3(f64::NAN, 1.0, 2.0).is_nan());
    }

    #[test]
    fn hypot_all_zero_is_positive_zero() {
        let z = hypot3(0.0, -0.0, 0.0);
        assert_eq!(z, 0.0);
        assert!(z.is_sign_positive());
    }

    #[test]
    fn hypot3_differs_from_the_plain_root_that_shipped_in_two_modules() {
        // A triple where the plain root really is one ULP off, so the
        // difference is visible here without running Node.
        //
        // The first draft of this test used `(0.1, 0.2, 0.3)`, chosen by eye on
        // the assumption that any old triple would do. It does not: those three
        // agree exactly, so the test asserted a coincidence rather than the
        // property, and it failed the moment it was first run. This triple is
        // one of 1,667 disagreements found by scanning
        // `tests/jsmath/golden.json` for cases where `sqrt(x*x + y*y + z*z)`
        // and V8 differ bit-for-bit — the only honest way to choose one.
        //
        // V8 gives 16.276242716028424; the plain root gives 16.27624271602842.
        let (x, y, z) = (
            8.907_641_209_661_96_f64,
            -9.805_145_198_479_295,
            9.456_697_767_600_417,
        );
        let plain = (x * x + y * y + z * z).sqrt();
        assert_ne!(
            hypot3(x, y, z).to_bits(),
            plain.to_bits(),
            "if these agree the sample no longer demonstrates the difference; \
             pick another from tests/jsmath/golden.json",
        );
    }

    #[test]
    fn sign_is_three_valued_unlike_signum() {
        assert_eq!(sign(0.0), 0.0);
        assert!(sign(0.0).is_sign_positive());
        assert_eq!(sign(-0.0), 0.0);
        assert!(sign(-0.0).is_sign_negative(), "Math.sign(-0) is -0");
        assert!(sign(f64::NAN).is_nan());
        assert_eq!(sign(3.5), 1.0);
        assert_eq!(sign(-3.5), -1.0);
        // The divergence this function exists to prevent.
        assert_eq!(0.0_f64.signum(), 1.0);
    }

    #[test]
    fn round_breaks_ties_toward_positive_infinity_unlike_f64_round() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0, "Math.round(-2.5) is -2");
        assert_eq!((-2.5_f64).round(), -3.0, "the divergence this exists to prevent");
        assert_eq!(round(0.5), 1.0);
        let neg_half = round(-0.5);
        assert_eq!(neg_half, 0.0);
        assert!(neg_half.is_sign_negative(), "Math.round(-0.5) is -0, not +0");
        assert!(round(-0.2).is_sign_negative(), "Math.round(-0.2) is -0");
        assert!(round(f64::NAN).is_nan());
        assert_eq!(round(f64::INFINITY), f64::INFINITY);
    }

    #[test]
    fn or_one_treats_nan_as_falsy_the_way_javascript_does() {
        assert_eq!(or_one(0.0), 1.0);
        assert_eq!(or_one(-0.0), 1.0);
        assert_eq!(or_one(f64::NAN), 1.0);
        assert_eq!(or_one(2.5), 2.5);
    }

    /// `|| fallback` replaces a falsy value; `max(fallback)` raises a small one.
    /// They are different functions and the port has already conflated them once.
    #[test]
    fn or_is_a_falsy_replace_and_not_a_clamp() {
        assert_eq!(or(0.0, 1e-4), 1e-4);
        assert_eq!(or(f64::NAN, 1e-4), 1e-4);
        // The case that separates them: truthy, but below the fallback.
        assert_eq!(or(5e-5, 1e-4), 5e-5);
        assert_eq!(5e-5_f64.max(1e-4), 1e-4, "the clamp this is not");
    }
}
