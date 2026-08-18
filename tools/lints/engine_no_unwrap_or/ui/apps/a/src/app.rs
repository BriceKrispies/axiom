// This fixture's path contains `apps/`, not `crates/` or `modules/`, so it is a
// composition leaf outside the engine spine: the lint must NOT fire even on a
// plain non-test `.unwrap_or(..)`. (Expected output: empty.)
#![allow(dead_code)]

fn app_code_may_unwrap_or() {
    let v: Option<i32> = None;
    let _ = v.unwrap_or(0);
}

fn main() {}
