// This fixture's path contains `apps/`, not `crates/` or `modules/`, so it is a
// composition leaf outside the engine spine: the lint must NOT fire even on plain
// uninit/zeroed calls. (Expected output: empty — no .stderr file.)
#![allow(dead_code)]

fn app_may_use_zeroed() {
    let _: u32 = unsafe { std::mem::zeroed() };
}

fn app_may_use_uninit() {
    let _ = core::mem::MaybeUninit::<u32>::uninit();
}

fn app_may_call_assume_init() {
    let mu = core::mem::MaybeUninit::<u32>::uninit();
    let _ = unsafe { mu.assume_init() };
}

fn main() {}
