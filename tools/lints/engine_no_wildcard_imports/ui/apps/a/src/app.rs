// This fixture's path contains `apps/`, not `crates/` or `modules/`, so it is a
// composition leaf outside the engine spine: the lint must NOT fire even on a
// plain glob import. (Expected output: empty.)
#![allow(unused_imports)]

use std::collections::*;

fn main() {}
