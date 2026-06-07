// This fixture's path contains `apps/`, not `crates/` or `modules/`, so it is a
// composition leaf outside the engine spine: the lint must NOT fire even on a
// public fn with a naked float parameter. (Expected output: empty.)
#![allow(dead_code)]

pub fn set_speed(speed: f32) {}

fn main() {}
