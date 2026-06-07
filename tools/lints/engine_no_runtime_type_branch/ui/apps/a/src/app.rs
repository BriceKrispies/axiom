// Path contains `apps/`, not `crates/` or `modules/`, so this is a composition
// leaf outside the engine spine: the lint must NOT fire even on a plain
// non-test downcast_ref or TypeId::of. (Expected output: empty.)
#![allow(dead_code)]

use std::any::{Any, TypeId};

fn app_code_may_downcast(value: &dyn Any) {
    if let Some(n) = value.downcast_ref::<u32>() {
        let _ = n;
    }
}

fn app_code_may_use_type_id() {
    let _ = TypeId::of::<u32>();
}

fn main() {}
