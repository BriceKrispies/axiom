// compile-flags: --test
// This fixture's path contains `modules/`, so it is treated as engine code: a
// directly self-recursive non-test fn here MUST be flagged. A non-recursive fn
// calling a DIFFERENT fn must NOT be flagged, and a recursive `#[test]` fn must
// NOT be flagged (proving the test exemption).
#![allow(dead_code)]

// ---- engine code: FLAGGED ----

// Plain function self-call.
fn down(n: u32) -> u32 {
    if n == 0 { 0 } else { down(n - 1) }
}

// Method self-call dispatches to the same DefId — also flagged.
struct Walker {
    depth: u32,
}

impl Walker {
    fn descend(&self) -> u32 {
        if self.depth == 0 {
            0
        } else {
            Walker { depth: self.depth - 1 }.descend()
        }
    }
}

// ---- engine code: NOT flagged ----

fn leaf(n: u32) -> u32 {
    n + 1
}

// Calls a DIFFERENT function, not itself.
fn caller(n: u32) -> u32 {
    leaf(n)
}

// ---- test code in an engine file: NOT flagged ----

// A recursive `#[test]` fn is exempt — proves the test exemption. (`#[test]`
// fns take no args, so it recurses on captured-free state and bails immediately.)
#[test]
fn a_recursive_test() {
    fn rec(n: u32) -> u32 {
        if n == 0 { 0 } else { rec(n - 1) }
    }
    let _ = rec(0);
}

#[cfg(test)]
mod tests {
    // Even a non-`#[test]` helper inside a `#[cfg(test)]` module is exempt.
    fn a_cfg_test_helper(n: u32) -> u32 {
        if n == 0 { 0 } else { a_cfg_test_helper(n - 1) }
    }
}
