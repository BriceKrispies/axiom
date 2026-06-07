// This fixture's path contains `apps/`, not `crates/` or `modules/`, so it is a
// composition leaf outside the engine spine: the lint must NOT fire even on a
// plain directly-recursive fn. (Expected output: empty.)
#![allow(dead_code)]

fn app_code_may_recurse(n: u32) -> u32 {
    if n == 0 { 0 } else { app_code_may_recurse(n - 1) }
}

fn main() {}
