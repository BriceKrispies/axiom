// Rule 12 (`unsafe-state-escape`): every mechanism that can bypass the state law.
#![allow(dead_code, unused)]

// ---- FLAGGED: an unsafe block ----

pub fn read_it(value: &u32) -> u32 {
    unsafe { *value }
}

// ---- FLAGGED: an unsafe fn ----

pub unsafe fn trust_me() {}

// ---- FLAGGED: an unsafe trait and an unsafe impl ----

pub unsafe trait Contiguous {}

pub struct Buffer;

unsafe impl Contiguous for Buffer {}

// ---- FLAGGED: raw pointers in an engine API ----

pub fn from_raw(ptr: *const u32) -> u32 {
    0
}

pub fn to_raw() -> *mut u32 {
    std::ptr::null_mut()
}

// ---- FLAGGED: a raw pointer in state-bearing machinery ----

pub struct RawHolder {
    ptr: *mut u32,
}

// ---- FLAGGED: an extern block ----

unsafe extern "C" {
    pub fn host_now() -> u64;
    pub static HOST_EPOCH: u64;
}

fn main() {}
