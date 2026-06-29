//! Proof that a Rust test can be **executed** on `wasm32` (not merely compiled)
//! and produce the same result it does natively — the foundation a later
//! workstream needs to run a physics-determinism golden on wasm and diff it
//! against native (today CI only `cargo build`s wasm, never runs it).
//!
//! The kernel below uses only the cross-target-portable float subset
//! `{+, -, *, /, sqrt}` — exactly the ops the `engine_no_unportable_float`
//! dylint enforces in the step path. Because that subset is bit-identical on
//! wasm32 and the native SSE2/NEON backends, the tests assert *exact* results
//! (including `to_bits`) and pass on both targets. Run both with
//! `scripts/wasm-test.ps1` (or `.sh`).

/// One fixed-step semi-implicit Euler integration of a particle under constant
/// acceleration, using only `{+, -, *}`. Deterministic and FMA-free.
#[must_use]
pub fn euler_step(position: f32, velocity: f32, acceleration: f32, dt: f32) -> (f32, f32) {
    let new_velocity = velocity + acceleration * dt;
    let new_position = position + new_velocity * dt;
    (new_position, new_velocity)
}

/// Vector magnitude via `sqrt`, which IEEE-754 mandates be correctly rounded —
/// hence bit-identical on every target.
#[must_use]
pub fn magnitude(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

#[cfg(test)]
mod tests {
    use super::{euler_step, magnitude};

    // On `wasm32` these run under `wasm-bindgen-test-runner` (node); on every
    // other target they are ordinary `#[test]`s. Same assertions, both targets.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn magnitude_is_exact_and_portable() {
        // 3-4-5 and 5-12-13 are exact in f32; sqrt is correctly rounded.
        assert_eq!(magnitude(3.0, 4.0), 5.0);
        assert_eq!(magnitude(5.0, 12.0), 13.0);
        // Asserting on the raw bit pattern is the strongest cross-target claim.
        assert_eq!(magnitude(3.0, 4.0).to_bits(), 5.0_f32.to_bits());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn euler_step_is_deterministic() {
        // Exactly-representable inputs keep the expected values exact in f32.
        let (p, v) = euler_step(2.0, 1.0, -8.0, 0.5);
        assert_eq!(v, -3.0); // 1.0 + (-8.0 * 0.5)
        assert_eq!(p, 0.5); //  2.0 + (-3.0 * 0.5)

        // Replaying the same inputs yields a byte-equal result.
        let again = euler_step(2.0, 1.0, -8.0, 0.5);
        assert_eq!((p.to_bits(), v.to_bits()), (again.0.to_bits(), again.1.to_bits()));
    }
}
