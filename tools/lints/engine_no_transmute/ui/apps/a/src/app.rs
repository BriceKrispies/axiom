// This fixture's path contains `apps/`, not `crates/` or `modules/`, so it is a
// composition leaf outside the engine spine: the lint must NOT fire even on a
// plain non-test `mem::transmute`. (Expected output: empty.)
#![allow(dead_code, unnecessary_transmutes)]

fn app_may_transmute() {
    let bits: u32 = 0x3f80_0000;
    let _f: f32 = unsafe { std::mem::transmute::<u32, f32>(bits) };
}

fn main() {}
