// Rule 1 (`static-storage`) and rule 2 (`thread-local-storage`).
//
// The path contains `modules/.../src`, so this file is engine spine code: every
// `static` here MUST be flagged, immutable ones included. `const` must not be.
#![allow(dead_code, unused)]

use std::cell::Cell;
use std::sync::OnceLock;

struct Cache;

// ---- FLAGGED: static-storage ----

// A plain immutable static is still process-wide storage.
static VALUE: u32 = 1;

// A `&str` static: same storage, no mutation needed to break the law.
static NAME: &str = "foo";

// The classic lazy-initialization slot.
static CACHE: OnceLock<Cache> = OnceLock::new();

// `static mut` is the loudest form (also covered by `engine_no_static_mut`).
static mut COUNTER: u32 = 0;

// ---- FLAGGED: thread-local-storage ----

thread_local! {
    static TICK: Cell<u32> = const { Cell::new(0) };
}

// ---- NOT flagged: `const` is a compile-time value, not storage ----

const MAX_PLAYERS: usize = 64;
const FIXED_STEP_NS: u64 = 16_666_667;

fn main() {}
