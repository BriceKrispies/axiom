// This fixture's path contains `apps/`, not `crates/` or `modules/`, so it is
// a composition leaf outside the engine spine: the lint must NOT fire even on
// an undocumented `pub mod`. (Expected output: empty.)
#![allow(dead_code)]

pub mod undocumented {}

fn main() {}
