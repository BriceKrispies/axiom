// compile-flags: --test
// This fixture's path contains `modules/`, so it is treated as engine code: a
// non-test `.unwrap()` here MUST be flagged. Test code, `.expect(..)`, and the
// non-panicking `unwrap_or*` combinators must NOT be.
#![allow(dead_code)]

// ---- engine code: FLAGGED ----

fn flagged_unwrap() {
    let v: Result<i32, ()> = Ok(1);
    let _ = v.unwrap();
}

fn flagged_unwrap_err() {
    let v: Result<(), i32> = Err(2);
    let _ = v.unwrap_err();
}

// ---- engine code: NOT flagged ----

// `.expect(..)` is the documented escape hatch.
fn allowed_expect() {
    let v: Result<i32, ()> = Ok(1);
    let _ = v.expect("ok by construction");
}

// The non-panicking combinators are fine.
fn allowed_combinators() {
    let v: Option<i32> = None;
    let _ = v.unwrap_or(0);
    let _ = v.unwrap_or_else(|| 0);
    let _ = v.unwrap_or_default();
}

// ---- test code in an engine file: NOT flagged ----

#[test]
fn a_test_function_may_unwrap() {
    let v: Result<i32, ()> = Ok(1);
    assert_eq!(v.unwrap(), 1);
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_cfg_test_test_may_unwrap() {
        let v: Result<i32, ()> = Ok(1);
        assert_eq!(v.unwrap(), 1);
    }

    // Even a non-`#[test]` helper inside a `#[cfg(test)]` module is exempt.
    fn a_cfg_test_helper_may_unwrap() {
        let v: Result<i32, ()> = Ok(1);
        let _ = v.unwrap();
    }
}
