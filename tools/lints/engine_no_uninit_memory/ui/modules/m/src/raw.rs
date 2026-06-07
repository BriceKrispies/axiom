// This fixture's path contains `modules/.../src`, so it is engine code.
// Calls to zeroed/uninitialized/MaybeUninit APIs here MUST be flagged.
// Test code is exempt.
#![allow(dead_code)]

// ---- engine code: FLAGGED ----

fn flagged_zeroed() {
    let _: u32 = unsafe { std::mem::zeroed() };
}

fn flagged_uninit() {
    let _ = core::mem::MaybeUninit::<u32>::uninit();
}

fn flagged_maybeuninit_zeroed() {
    let _ = core::mem::MaybeUninit::<u32>::zeroed();
}

fn flagged_assume_init() {
    let mu = core::mem::MaybeUninit::<u32>::uninit();
    let _ = unsafe { mu.assume_init() };
}

// ---- test code in an engine file: NOT flagged ----

#[test]
fn test_may_use_uninit() {
    let _: u32 = unsafe { std::mem::zeroed() };
    let _ = core::mem::MaybeUninit::<u32>::uninit();
}

fn main() {}
