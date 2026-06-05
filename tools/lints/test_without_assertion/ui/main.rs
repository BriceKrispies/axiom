// compile-flags: --test
// UI fixtures for the `test_without_assertion` lint. Compiled with `--test` so
// `#[test]` is in effect — exactly how the lint runs against the real repo's
// test targets. Each `#[test]` below is either a case the lint MUST flag (no
// assertion under the STRICT policy) or MUST leave alone (has one).
#![allow(dead_code, unused_variables, clippy::eq_op, clippy::unnecessary_literal_unwrap)]

// ---- SHOULD be flagged: no assertion at all ----

#[test]
fn no_assertion_just_executes() {
    let _ = 1 + 1;
}

// Mirrors the audit's real findings (`new_and_default_*_are_equivalent`): binds
// constructors to `_`, asserts nothing. (Named without the `ui` substring so the
// compiletest path normalizer doesn't rewrite it in the snapshot.)
#[test]
fn new_and_default_only_construct() {
    let _ = String::new();
    let _ = String::default();
}

// Mirrors `all_accessors_are_reachable`: calls accessors, discards results.
#[test]
fn all_accessors_are_reachable() {
    let s = String::from("hi");
    let _ = s.len();
    let _ = s.is_empty();
    let _ = s.capacity();
}

// STRICT: a bare `.unwrap()` is not an assertion — it proves the value wasn't an
// error, not that behavior is correct.
#[test]
fn unwrap_only_is_flagged() {
    let v: Result<i32, ()> = Ok(3);
    v.unwrap();
}

// STRICT: a bare `?` is not an assertion either.
#[test]
fn question_mark_only_is_flagged() -> Result<(), String> {
    Err::<(), _>("boom".to_string()).map_err(|e| e)?;
    Ok(())
}

// Delegates to a helper that does NOT assert -> still flagged.
#[test]
fn delegates_to_non_asserting_helper() {
    just_compute(3);
}

fn just_compute(_n: i32) -> i32 {
    0
}

// ---- SHOULD NOT be flagged: a real assertion is present ----

#[test]
fn has_assert_eq() {
    assert_eq!(1 + 1, 2);
}

#[test]
fn has_assert() {
    assert!(!"axiom".is_empty());
}

#[test]
#[should_panic]
fn should_panic_is_its_own_assertion() {
    let v: Option<i32> = None;
    v.unwrap();
}

#[test]
fn assertion_inside_a_closure() {
    [1, 2, 3].iter().for_each(|&x| assert!(x > 0));
}

// Helper whose NAME contains "assert" — trusted without inspecting its body.
#[test]
fn uses_a_custom_assert_helper() {
    assert_things(3);
}

fn assert_things(_n: i32) {}

// SEMANTIC: the helper's name has no "assert", but the lint follows the call into
// its body and finds the `assert_eq!` — so this test is NOT flagged.
#[test]
fn delegates_to_an_asserting_helper() {
    check_roundtrip(7);
}

fn check_roundtrip(n: i32) {
    assert_eq!(n, n);
}

// Declares a helper trait impl inline (its `run` has no assertion) and asserts at
// the end. The inner `run` is inside a test but is NOT the test — it must not be
// flagged, and this test itself asserts, so nothing here should warn.
#[test]
fn test_with_inner_impl_method_is_left_alone() {
    struct Sys;
    trait Run {
        fn run(&self) -> i32;
    }
    impl Run for Sys {
        fn run(&self) -> i32 {
            0
        }
    }
    assert_eq!(Sys.run(), 0);
}

// A plain (non-test) function with no assertion must never be flagged.
fn not_a_test_function() {
    let _ = 5;
}
