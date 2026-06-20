# Hardening the TypeScript SDK to the engine's laws

`packages/axiom-client` (the `@axiom/client` multiplayer browser SDK) is being
brought up to TypeScript-native versions of the Rust engine spine's laws:
maximum-strictness static analysis, the Branchless Law, and 100% coverage. This
is the TS counterpart of `docs/unbranching.md`.

**Phase 1 (tooling) is done** — the gates exist, run, and report at full
strength (see `packages/axiom-client/STATIC_ANALYSIS.md`). This document tracks
**Phase 2/3 (remediation to green)**: the backlog the gates surface, and how to
clear it. Until it is green, the gate is deliberately not wired into the blocking
pre-commit hook or CI (mirroring the Rust `--no-verify` unbranching loop).

## Current backlog (baseline snapshot)

Run `npm --prefix packages/axiom-client run lint` and `… run coverage` for the
live numbers. At Phase 1 close:

| Gate                          | Status |
|-------------------------------|--------|
| `tsgo` typecheck              | ✅ green |
| Oxlint (all categories error) | ❌ 454 findings in `src/`, 301 in `test/` |
| Branch ban (subset of above)  | ❌ 53 findings in `src/` (`if`, `?:`, `switch`, `for`/`for...of`, `&&`/`||`/`??`, `?.`) |
| Coverage (100%)               | ❌ 85.7% lines / 85.9% branches / 83.2% functions |

Coverage gaps concentrate in `transport.ts` (52% lines — the WebTransport/WebRTC
async arms are untested) and the `client.ts` message-dispatch arms.

Top non-branch lint rules by volume: `prefer-readonly-parameter-types` (55),
`no-magic-numbers` (42), `explicit-member-accessibility` (39), `id-length` (38),
`func-style` (29), plus the `no-unsafe-*` / `no-floating-promises` /
`switch-exhaustiveness-check` type-aware family.

## Workstreams

### 1. Pass the branch ban (the Branchless Law)

Rewrite `client.ts`, `protocol.ts`, `transport.ts` branchlessly using the recipe
catalog below. The shapes here map onto the Rust recipes in
`docs/unbranching.md`:

- **Message dispatch** (`switch (message.kind)` in `client.ts`, `peekKind`
  dispatch in `protocol.ts`) → a `Record<Kind, handler>` / `Map` keyed by the
  discriminant, indexed instead of branched.
- **Validation guards** (`if (bytes.length === 0 || …) throw`) → a small
  `Result`/`Option`-style value transform, or a table of validators applied with
  `.every`/`.find`, instead of early-throw `if`.
- **State guards** (`if (this.status !== "connected") return null`) → encode the
  decision as a value (`statusRank[...]`) and select, instead of an early return.
- **Byte loops** (the `for` codec loops) → `Uint8Array.from` / `map` / `reduce`
  over indices.
- **Defaults** (`config.token ?? new Uint8Array()`, `x ? a : b`) → arithmetic /
  table selection / a tiny `defaulted(value, fallback)` helper.

### 2. Pass the rest of Oxlint

Fix every remaining finding. Most are mechanical (add `readonly`, member
accessibility, named constants, explicit return types). Two need a **judgment
call, recorded here, not a silent suppression**:

- **`no-bitwise`** — the wire codec (`protocol.ts`) is fundamentally bit/byte
  arithmetic (little-endian packing). This is a `restriction`-category *opinion*,
  not a correctness rule, and it is structurally wrong for a binary protocol.
  Decision: reconsider this one rule for `protocol.ts` during Phase 2 (justified
  in the config with a comment) — the codec needs bitwise ops the way it needs
  numbers. This is *not* a precedent for relaxing correctness/suspicious rules.
- **`no-magic-numbers`** — wire offsets/sizes should become named constants
  (genuine improvement), not be exempted.

### 3. Reach 100% coverage (the Coverage Law)

The async transport arms are the hard part — see the design signal below. Add
tests for every message-dispatch arm and codec path; restructure any genuinely
untestable shape rather than adding coverage theater (same rule as the Rust
Coverage Law).

### 4. Make the gate hard

When `make ts-gate` is green: add it to `.git/hooks/pre-commit` (a 4th step
beside architecture/coverage/dylint) and as a CI step in
`.github/workflows/ci.yml`. Flip the branch ban to a permanent baseline-0
invariant.

## Design signal: the async transport layer

`transport.ts` (WebSocket/WebTransport/WebRTC) is built on `async`/`await`,
`try`/`catch`, and Promise flow, which have no clean branchless form — and
type-aware lint (`no-floating-promises`, `promise-function-async`,
`no-unsafe-*` around the DOM streaming APIs) bites hardest here. This is the same
question the Rust spine answered by scoping `engine_no_branching` out of apps and
the platform `host` edge.

**To settle in Phase 2, before mass-rewriting:** decide whether `transport.ts` is
the SDK's "platform edge" (the browser-API boundary, scoped out of the branch ban
the way `host`/`windowing` are on the Rust side — the protocol codec and client
state machine stay fully branchless), or whether a small async-result combinator
removes the branches without an exemption. Do not resolve it by quietly disabling
the rule per-line. Pick the structural answer and write it down here.

## Branchless recipe catalog (TypeScript)

| Branchy form | Branchless rewrite |
|--------------|--------------------|
| `cond ? a : b` | `[b, a][Number(cond)]` |
| `if (cond) { stmt }` | `[() => {}, () => stmt][Number(cond)]()` or `[stmt-table][key]` |
| `if (cond) return x; …` | hoist to a value; select the return with a table |
| `switch (k) { case … }` | `({ [K1]: h1, [K2]: h2 }[k] ?? noop)(…)` → indexed record (the default itself table-selected, not `??`) |
| `for (let i…) acc += f(i)` | `Array.from({length}, (_, i) => f(i)).reduce((a, b) => a + b, 0)` |
| `for (const x of xs) g(x)` | `xs.forEach(g)` / `xs.map(g)` |
| `a && b` (b pure) | `a & b` (booleans) / `Number(a) * Number(b)` |
| `a \|\| b` (b pure) | `a \| b` / `Math.max` / table |
| `x ?? d` | `defaulted(x, d)` value transform, or `[d, x][Number(x !== undefined)]` |
| `obj?.prop` | make presence explicit: `[fallback, obj.prop][Number(obj !== null)]` behind a guarded accessor |

Tests are exempt from the branch ban (the `overrides` block in `.oxlintrc.json`),
exactly as Rust tests keep their branches — never rewrite a test to satisfy it.
