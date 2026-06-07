// Path contains `apps/`, not `crates/` or `modules/`, so it is a composition
// leaf outside the engine spine: the lint must NOT fire even on a `static mut`.
#![allow(dead_code)]

static mut COUNTER: u32 = 0;

fn main() {}
