// Path contains `modules/.../src`, so this is engine code.
// Both spawn forms MUST be flagged; test code must NOT be flagged.
#![allow(dead_code)]

// ---- engine code: FLAGGED ----

fn flagged_free_spawn() {
    let _ = std::thread::spawn(|| {});
}

fn flagged_builder_spawn() {
    let handle = std::thread::Builder::new()
        .name("worker".to_string())
        .spawn(|| {})
        .expect("thread spawn failed");
    handle.join().unwrap();
}

// ---- test code in an engine file: NOT flagged ----

#[test]
fn test_may_spawn() {
    let h = std::thread::spawn(|| 42);
    assert_eq!(h.join().unwrap(), 42);
}

fn main() {}
