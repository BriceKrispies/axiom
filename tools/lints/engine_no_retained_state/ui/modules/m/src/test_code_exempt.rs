// Tests are out of scope: the law constrains the engine the build ships, not the
// suites that verify it. This file is engine spine code, but every construct
// below sits inside a `#[test]` fn or a `#[cfg(test)]` module, so it MUST
// produce zero findings — there is no `.stderr` beside it.
#![allow(dead_code, unused)]

#[test]
fn a_test_may_retain_state() {
    use std::cell::RefCell;
    use std::sync::Arc;

    let scratch = RefCell::new(0_u32);
    let shared = Arc::new(1_u32);
    let raw = unsafe { *(&1_u32 as *const u32) };
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    pub struct Harness {
        pub captured: Mutex<Vec<u32>>,
    }

    pub fn record(harness: &mut Harness, value: u32) {}
}

fn main() {}
