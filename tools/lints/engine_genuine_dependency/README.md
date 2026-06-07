# `engine_genuine_dependency`

A [dylint] lint enforcing half of the Axiom Layer Law: **every layer's declared
`depends_on` must be genuinely used.**

For a layer crate (one with a `layer.toml`), it flags any `depends_on` entry the
crate never references in **non-test** code. The `xtask` checker owns the graph's
*shape* (edges are real, the graph is acyclic) from `cargo metadata` + the
manifests; this lint, with real `DefId`/type information, owns the *genuine-use*
half that a text scan cannot prove.

### Why

A `depends_on` entry a layer does not use is a **ceremonial dependency** — it
fakes an adapter relationship the code does not have. Axiom does not take
shortcuts; if you stop using a dependency, remove it from `depends_on` and
`Cargo.toml` rather than leaving a dead edge to satisfy the graph.

### What it cannot prove

Whether a *referenced* dependency is used *meaningfully* versus ceremonially — a
single trivial call counts as "used." That judgment lives in
`meaningful_dependency` prose and review. This lint guarantees the declared edge
is real; it cannot read intent.

### Scope

- Only layer crates (those with a `layer.toml`) are checked; modules, apps,
  tooling, and the kernel root (empty `depends_on`) are skipped.
- Test code (`#[test]` / `#[cfg(test)]`) does not count as genuine use — a
  dependency used only in tests is still flagged.

### Running it

```sh
cargo dylint --all -- --all-targets
```

[dylint]: https://github.com/trailofbits/dylint
