// Path contains `apps/`, not `crates/` or `modules/`, so this is a composition
// leaf outside the engine spine: the lint must NOT fire here.
#![allow(dead_code)]

fn app_code_may_spawn() {
    let _ = std::thread::spawn(|| {});
}

fn main() {}
