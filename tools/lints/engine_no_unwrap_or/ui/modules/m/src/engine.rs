// compile-flags: --test
// This fixture's path contains `modules/`, so it is treated as engine code: a
// non-test `.unwrap_or(..)` here MUST be flagged. The lazy siblings, `.expect`,
// and test code must NOT be.
#![allow(dead_code)]

// ---- engine code: FLAGGED ----

fn flagged_on_option() {
    let v: Option<i32> = None;
    let _ = v.unwrap_or(0);
}

fn flagged_on_result() {
    let v: Result<i32, ()> = Ok(1);
    let _ = v.unwrap_or(0);
}

// ---- engine code: NOT flagged ----

// Only the eager, value-taking `unwrap_or` is in scope for this lint.
fn allowed_lazy_siblings() {
    let v: Option<i32> = None;
    let _ = v.unwrap_or_else(|| 0);
    let _ = v.unwrap_or_default();
}

// A method that merely *starts* with `unwrap_or` is not this method.
struct Fallback;

impl Fallback {
    fn unwrap_ordinal(&self) -> i32 {
        0
    }
}

fn allowed_similar_name() {
    let _ = Fallback.unwrap_ordinal();
}

// ---- test code in an engine file: NOT flagged ----

#[test]
fn a_test_function_may_unwrap_or() {
    let v: Option<i32> = None;
    assert_eq!(v.unwrap_or(7), 7);
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_cfg_test_test_may_unwrap_or() {
        let v: Option<i32> = None;
        assert_eq!(v.unwrap_or(7), 7);
    }

    // Even a non-`#[test]` helper inside a `#[cfg(test)]` module is exempt.
    fn a_cfg_test_helper_may_unwrap_or() {
        let v: Option<i32> = None;
        let _ = v.unwrap_or(7);
    }
}
