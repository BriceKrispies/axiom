# `axiom-agent` — Testing

`axiom-agent` is held to Axiom's **Coverage Law** (every region, line, branch, and
function exercised — 100%, always) and the **Branchless Law** (non-test code
contains zero control flow). This document records *how* the module is tested and
the principles its tests will not violate.

## The three test surfaces

1. **Inline `#[cfg(test)] mod tests`** in every `src/*.rs` except the pure
   re-export `lib.rs`. These name the crate-internal types directly and drive the
   sealed contracts the external test crate cannot name: each constructor, each
   accessor, each branchless arm. Where a value is selected branchlessly
   (`[a, b][cond as usize]`, `cond.then(..)`, `take(max)`, `find(..)`), the tests
   reach the site with the condition both true and false so every region is hit.

2. **`tests/agent_api_tests.rs`** — behavioral proofs driven **only** through the
   public `AgentApi` facade. Sealed return values are bound by type inference and
   asserted through their public accessors; no sealed type is ever named. This is
   the determinism and boundary surface.

3. **`tests/architecture.rs`** — the structural/hygiene gate (see below).

## Deterministic brain tests

- **Scripted:** an empty brain emits `Noop` (`no_matching_rule`); a matching fact
  emits that rule's configured intent **and its rule-carried reason code**; the
  **first** matching rule in order wins; a non-matching observation falls back to
  `Noop`; the emitted count never exceeds the profile's `max_actions_per_tick`;
  and a **zero-budget profile emits no action at all**, reported as
  `action_budget_zero` — driven through the facade's
  `profile_with_action_budget`.
- **Replay:** an empty recording emits `Noop` reported as `replay_empty`;
  recorded intents are emitted in order (`replay_emitted`); stepping past the end
  emits `Noop` reported as `replay_complete` — empty and completed are distinct,
  separately asserted reasons. The cursor advance makes the emitted sequence a
  pure function of the recording.

Both are exercised end-to-end through `AgentRuntime`/`AgentApi::step` as well as
directly, so the generic step is covered for each brain kind.

## Bounded observation tests

The builder preserves channel/fact/legal-action insertion order, builds an
`Observation` whose accessors report exactly what was added, and **fails
deterministically** when each of its three bounds is exceeded — returning a kernel
`OutOfBounds` error in the `Memory` scope, never panicking. Both the accept arm
(room available) and the reject arm (bound reached) are tested for every `add_*`.

## Bounded action queue tests

The queue is proven FIFO (push to back, pop from front), reports `len` /
`is_empty` / `capacity`, returns `None` from `pop` when empty, and **fails
deterministically** on overflow with the same kernel `OutOfBounds` error. The
internal `from_intents` sizing used by the runtime is covered too.

## Memory tests

Memory preserves insertion order under capacity, **drops the oldest entry** when a
`remember` would exceed capacity (verified by inspecting the surviving keys/ticks),
stores nothing at capacity `0`, and clears to empty while keeping its bound.

## Decision-report stability tests

`DecisionReport` is all-numeric, so a decision is a comparable artifact. The
determinism proof builds two independent agents with identical inputs and asserts
**both** the report **and** the emitted action list are equal
(`identical_inputs_replay_to_identical_report_and_actions`). A second proof shows
that an observation matching a *different* rule yields a *different* — but still
deterministic — report (`a_different_matching_rule_yields_a_different_report`).

## Architecture / hygiene tests

`tests/architecture.rs` scans the `src/` tree (with comments and string literals
stripped, so prose can neither mask nor fabricate a hit) and fails on:

- a missing/under-specified `module.toml` (must be an isolated `engine-module`);
- `lib.rs` exporting anything other than the single `AgentApi` facade;
- importing any layer other than `axiom-kernel` / `axiom-runtime`, or any other
  module, or any app/tool;
- browser/JS/WebGPU/DOM/canvas APIs; wall-clock time; randomness; threads/async/
  net/process; console prints or placeholder macros; global mutable state;
  hash/btree/linked collections; junk-drawer module names;
- **prohibited AI concepts** — pathfinding, navmesh, behavior tree, utility-AI,
  planner, ML, neural, LLM (`no_prohibited_ai_concepts`), encoding the module's
  hard prohibitions as a build gate;
- any orphan `src/*.rs` not declared in `lib.rs`.

## Why "does not panic" is not an acceptable test

A test that merely calls code and checks nothing moves the coverage number while
proving nothing: it cannot fail when behavior regresses, and it rots into noise. A
determinism substrate is worthless if its outputs are not pinned, so **every test
here asserts a concrete value** — an exact intent kind, an exact ordered queue, an
exact dropped-oldest survivor set, an exact report, or an exact deterministic
error code. Coverage is a by-product of proving behavior, never the goal itself.
If a piece of code cannot be reached by a behavioral assertion, that is a design
signal to restructure — not a reason to add a coverage-only test.
