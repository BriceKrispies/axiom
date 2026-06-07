// This fixture's path contains `apps/`, not `crates/` or `modules/`, so it is
// a composition leaf outside the engine spine: the lint must NOT fire even when
// the struct has more than MAX_FIELDS fields. (Expected output: empty.)
#![allow(dead_code)]

struct AppGodStruct {
    f0: u8,
    f1: u8,
    f2: u8,
    f3: u8,
    f4: u8,
    f5: u8,
    f6: u8,
    f7: u8,
    f8: u8,
    f9: u8,
    f10: u8,
    f11: u8,
    f12: u8,
    f13: u8,
    f14: u8,
    f15: u8,
    f16: u8,
    f17: u8,
    f18: u8,
    f19: u8,
    f20: u8,
    f21: u8,
    f22: u8,
    f23: u8,
    f24: u8,
    f25: u8,
}

fn main() {}
