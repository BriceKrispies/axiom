# `test_without_assertion`

A [dylint] lint that flags any `#[test]` function whose body contains **no
assertion**. It enforces, mechanically, the part of Axiom's Coverage Law that
forbids "tests that execute code without asserting on its behavior" — coverage
theater that moves the gate without proving anything.

### What counts as an assertion (strict policy)

A test is considered to assert if it has any of:

- an `assert!` / `assert_eq!` / `assert_ne!` / `debug_assert*!` (or `panic!` /
  `unreachable!`) macro — anywhere in the body or a closure inside it, detected
  through the macro-expansion backtrace;
- the `#[should_panic]` attribute (the expected panic *is* the check);
- a call to a helper that asserts — either a helper whose name contains
  `assert`, or, resolved **semantically**, a local helper whose own body asserts
  (the lint follows the call up to a few levels deep).

A bare `.unwrap()` / `.expect()` or `?` does **not** count: it proves the value
wasn't the error variant, not that the behavior under test is correct.

### What is exempt

`#[ignore]`d tests are not flagged. The lint's target is coverage *theatre* — a
test that runs code and asserts nothing, moving the coverage number while
proving nothing. An ignored test never executes under `cargo test` or
`cargo-llvm-cov`, so it contributes zero coverage and cannot be theatre by
construction. What it usually is instead is an instrumentation probe kept in the
tree to be run by hand (`cargo test -- --ignored --nocapture`) to print a
diagnostic; asserting nothing is the point of it.

This is not a loophole. `#[ignore]` buys silence only by also guaranteeing the
test never runs, so hiding a real test behind it costs you the test entirely —
the escape is self-punishing.

### Running it

From the repo root (the workspace declares this lint in
`[workspace.metadata.dylint]`):

```sh
cargo dylint --all -- --all-targets
```

`--all-targets` matters: test code lives behind `#[cfg(test)]`, which a plain
`cargo check` does not compile, so without it the lint sees no tests.

### Tests

`cargo test` runs the `ui/` fixtures via `compiletest_rs`. `dylint_testing`
6.0.1 has no bless support, so to refresh the snapshot after changing the lint:
run `cargo test`, then copy the "Actual stderr saved to …" file over
`ui/main.stderr`.

[dylint]: https://github.com/trailofbits/dylint
