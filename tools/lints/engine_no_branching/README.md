# `engine_no_branching`

A dylint lint that **bans every form of branching / control flow in Rust**,
everywhere.

## What it flags

Every one of these is a finding, no matter where it appears:

| Construct | Detected as |
|---|---|
| `if`, `if let`, `let … else` | HIR `ExprKind::If` |
| `match` | `ExprKind::Match(_, _, Normal)` |
| `?` | `ExprKind::Match(_, _, TryDesugar)` |
| `while`, `while let` | `ExprKind::Loop(_, _, While)` |
| `for` | `ExprKind::Loop(_, _, ForLoop)` |
| `loop` | `ExprKind::Loop(_, _, Loop)` |
| `&&` | `ExprKind::Binary(And)` |
| `\|\|` | `ExprKind::Binary(Or)` |

Each surface construct is reported exactly once (a `for`/`while` desugaring's
inner `ForLoopDesugar` match is not double-counted).

## Scope, exemptions, enforcement

This lint is intentionally **maximal**, per its commissioning:

- **Scope: everything except test code.** No engine-file filter — it fires on
  every non-test file the build compiles (`crates/`, `modules/`, `apps/`,
  `xtask`, `axiom-zones`, `tools/axiom-netcode-relay`). `#[test]` functions and
  `#[cfg(test)]` modules are exempt: the ban targets the engine the build ships,
  not the suites that verify it. The `tools/lints/` platform is a separate cargo
  workspace, so the root `cargo dylint` run does not scan the lint's own source.
- **Zero exemptions.** Unlike the rest of the rulebook, this lint honors **no**
  zone marker — not `#[escape_hatch]`, not `#[supervisor]`, not `#[strict]`.
  Nothing annotated is spared.
- **Hard ban, baseline 0.** It is registered with no `dylint-baseline.txt`
  entry, so the pre-commit ratchet allows zero findings. The codebase contains
  hundreds of branches today, so **the dylint gate fails until branching is
  removed** — by design.

## What it does NOT flag

- **Macro-internal branching.** A branch that comes from a macro expansion
  (`assert!`, `matches!`, a user `macro_rules!`, etc.) is skipped, so diagnostics
  land on control flow the programmer wrote. Compiler desugarings of surface
  constructs (`for` / `while` / `?` / `if let`) carry a desugaring kind and are
  still flagged.
- **`async`/`.await` desugaring.** `.await` lowers to a generator poll loop
  (`loop { match poll { Ready => break, Pending => yield } }`); `async fn`/`async`
  blocks lower to generators. That loop/match is compiler machinery, not a
  branch the programmer wrote, so spans tagged `DesugaringKind::Async`/`Await`
  are skipped — just like a `for` loop's internal `next()` match. A real
  `if`/`match`/`loop` written *inside* an async fn is still flagged (it keeps its
  own non-desugared span). (If you ever want to ban `async`/`.await` itself,
  that's a separate rule — this lint is about source branching keywords.)
- **Combinators.** `.map()`, `.and_then()`, `.unwrap_or()`, `.filter()`,
  `.ok()`, `.is_some()`, etc. are method calls, not language branching
  constructs, so they are not flagged. ("All forms of branching in the Rust
  programming language" means the language's control-flow expressions, not the
  internals of library combinators.)

## Relationship to `engine_no_source_branching`

The rulebook backlog reserves `engine_no_source_branching` for a version scoped
to `#[strict]` zones only. This lint is the **global, ungated** counterpart: same
construct set, but everywhere, with no zone and no escape hatch.

## Running

```sh
# UI test (from this crate):
cargo test

# Across the whole workspace (from repo root):
cargo dylint --all -- --all-targets
```
