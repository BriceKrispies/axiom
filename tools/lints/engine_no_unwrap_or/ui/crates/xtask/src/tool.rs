// Path is `crates/xtask/src/…` — the `xtask` tool is repo tooling, outside the
// engine spine, so it is exempt even though it sits under `crates/` with a `src`
// component. The `unwrap_or` below must NOT be flagged.
#![allow(dead_code)]

fn tool_code_may_unwrap_or() {
    let v: Option<i32> = None;
    let _ = v.unwrap_or(0);
}

fn main() {}
