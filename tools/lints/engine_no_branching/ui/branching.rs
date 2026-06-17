// compile-flags: --test
// edition:2021
// Every control-flow construct below must be flagged by `engine_no_branching`.
// The straight-line function, the combinator calls, and the `#[cfg(test)]`
// module at the bottom must NOT be.
#![allow(unused, clippy::all)]

fn if_construct(x: i32) -> i32 {
    if x > 0 {
        1
    } else {
        2
    }
}

fn if_let_construct(o: Option<i32>) -> i32 {
    if let Some(v) = o {
        v
    } else {
        0
    }
}

fn match_construct(x: i32) -> i32 {
    match x {
        0 => 10,
        _ => 20,
    }
}

fn while_construct() {
    let mut i = 0;
    while i < 3 {
        i += 1;
    }
}

fn while_let_construct(mut v: Vec<i32>) {
    while let Some(_x) = v.pop() {}
}

fn for_construct() {
    for _i in 0..3 {}
}

fn loop_construct() {
    loop {
        break;
    }
}

fn and_construct(a: bool, b: bool) -> bool {
    a && b
}

fn or_construct(a: bool, b: bool) -> bool {
    a || b
}

fn try_construct(o: Option<i32>) -> Option<i32> {
    let v = o?;
    Some(v)
}

// --- must NOT be flagged: straight-line code + combinator method calls ---

fn straight_line(a: i32, b: i32) -> i32 {
    let c = a + b;
    c * 2
}

fn combinators(o: Option<i32>) -> i32 {
    o.map(|x| x + 1).unwrap_or(0)
}

// --- must NOT be flagged: branches inside test code are exempt ---

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_may_branch_freely() {
        for i in 0..3 {
            if i % 2 == 0 {
                assert!(i >= 0);
            }
        }
    }
}

// --- must NOT be flagged: async/await desugaring is compiler machinery, not
// written branching (the `.await` lowers to a poll loop) ---

async fn awaits<F: core::future::Future<Output = i32>>(f: F) -> i32 {
    f.await
}

fn main() {}
