// This fixture's path contains `apps/`, not `crates/` or `modules/`, so it is a
// composition leaf outside the engine spine: the lint must NOT fire even on a
// plain non-test `.unwrap()`. (Expected output: empty.)
#![allow(dead_code)]

fn app_code_may_unwrap() {
    let v: Result<i32, ()> = Ok(1);
    let _ = v.unwrap();
}

fn main() {}
