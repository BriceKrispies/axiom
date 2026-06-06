// Path is `crates/<x>/tests/…` — an integration test (no `src` component). The
// whole file is test code, but integration tests are NOT wrapped in
// `#[cfg(test)]`, so `is_in_test` can't see a non-`#[test]` helper here. The
// `src`-component requirement is what keeps it (and benches/examples) exempt:
// the unwrap below must NOT be flagged.
#![allow(dead_code)]

fn helper_in_an_integration_test() {
    let v: Result<i32, ()> = Ok(1);
    let _ = v.unwrap();
}

fn main() {}
