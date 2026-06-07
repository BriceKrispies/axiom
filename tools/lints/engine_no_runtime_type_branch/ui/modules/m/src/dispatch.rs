// compile-flags: --test
// Path contains `modules/.../src`, so this is engine code. The runtime type
// reflection calls below MUST be flagged. Test code is exempt.
#![allow(dead_code)]

use std::any::{Any, TypeId};

// ---- engine code: FLAGGED ----

fn flagged_downcast_ref(value: &dyn Any) {
    if let Some(n) = value.downcast_ref::<u32>() {
        let _ = n;
    }
}

fn flagged_type_id_of() {
    let _ = TypeId::of::<u32>();
}

// ---- engine code: NOT flagged ----

// Test functions may use reflection freely.
#[test]
fn test_may_use_downcast() {
    let value: &dyn Any = &42u32;
    assert!(value.downcast_ref::<u32>().is_some());
}

#[cfg(test)]
mod tests {
    use std::any::{Any, TypeId};

    #[test]
    fn cfg_test_may_downcast() {
        let value: &dyn Any = &1u32;
        assert_eq!(value.downcast_ref::<u32>(), Some(&1u32));
    }

    fn cfg_test_helper_may_use_type_id() {
        let _ = TypeId::of::<u32>();
    }
}
