// This fixture's path contains `modules/.../src/`, so it is treated as engine
// code. A `mem::transmute` or `mem::transmute_copy` call in non-test code MUST
// be flagged. The same call inside a `#[test]` fn must NOT be flagged.
#![allow(dead_code, unnecessary_transmutes)]

// ---- engine code: FLAGGED ----

fn reinterpret_bits() {
    let bits: u32 = 0x3f80_0000;
    // transmute a u32 bit-pattern as f32 — banned in engine code
    let _f: f32 = unsafe { std::mem::transmute::<u32, f32>(bits) };
}

fn copy_bits() {
    let val: u32 = 42;
    // transmute_copy is equally banned
    let _f: f32 = unsafe { std::mem::transmute_copy::<u32, f32>(&val) };
}

// ---- engine code: NOT flagged ----

// Safe typed alternative — fine.
fn safe_from_bits() {
    let bits: u32 = 0x3f80_0000;
    let _f = f32::from_bits(bits);
}

// `as` cast — fine.
fn safe_as_cast() {
    let x: u32 = 1;
    let _y = x as f32;
}

fn main() {}
