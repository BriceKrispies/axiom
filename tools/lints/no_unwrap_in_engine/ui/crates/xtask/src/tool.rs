// Path is `crates/xtask/src/…` — the `xtask` tool is repo tooling, outside the
// engine spine, so it is exempt even though it sits under `crates/` with a `src`
// component. The unwrap below must NOT be flagged.
#![allow(dead_code)]

fn tool_code_may_unwrap() {
    let v: Result<i32, ()> = Ok(1);
    let _ = v.unwrap();
}

fn main() {}
