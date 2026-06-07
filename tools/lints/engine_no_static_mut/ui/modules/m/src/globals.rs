// Path contains `modules/.../src`, so this is engine code.
// A `static mut` here MUST be flagged.
// A plain immutable `static` must NOT fire.
#![allow(dead_code)]

// ---- engine code: FLAGGED ----

static mut COUNTER: u32 = 0;

// ---- engine code: NOT flagged ----

// A plain immutable static is fine.
static OK: u32 = 0;

fn main() {}
