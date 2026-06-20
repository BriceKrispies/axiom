# Hardening the TypeScript SDK to the engine's laws

`packages/axiom-client` (the `@axiom/client` multiplayer browser SDK) is being
brought up to TypeScript-native versions of the Rust engine spine's laws:
maximum-strictness static analysis, the Branchless Law, and 100% coverage. This
is the TS counterpart of `docs/unbranching.md`.

**Done — the SDK is green.** Phase 1 stood up the gates; Phase 2/3 rewrote the
SDK to pass them. `tsgo` typecheck, Oxlint (every category an error + the branch
ban), and `node:test` 100% coverage all pass, and the gate is wired into the
pre-commit hook and CI. This document records how it was done; the tool↔law
mapping and the documented exceptions live in
`packages/axiom-client/STATIC_ANALYSIS.md`.

## Where it started (baseline snapshot)

At Phase 1 close the gates reported:

| Gate                          | Then | Now |
|-------------------------------|------|-----|
| `tsgo` typecheck              | ✅   | ✅  |
| Oxlint (all categories error) | ❌ 454 in `src/`, 301 in `test/` | ✅ 0 |
| Branch ban (subset)           | ❌ 53 in `src/` | ✅ 0 |
| Coverage (100%)               | ❌ 85.7% lines | ✅ 100% |

## How the branchless rewrite was done

The enabling primitive is the **TypeScript assertion function** (`asserts cond`),
which narrows types with no `if`/`?:`/`as`/`!`:

- **Validation + dispatch** → `assert(cond, msg)` (branchless: `[msg].slice(Number(cond)).map(fail)`),
  and `assertKind` (`asserts raw is DecodedKind`) over a `Set`, so `peekKind`
  returns a narrowed kind and `decodeFrame` indexes a total `Record` instead of a `switch`.
- **Selection** → `pick(options, index)` (a generic `asserts value is Value` gated
  on an in-range numeric check) and `coalesce(value, fallback)` (array-destructuring
  default, which applies exactly on `undefined`).
- **Side-effect iteration** → `each` (a `.map` with a constant return — `.forEach`
  is banned by `no-array-forEach`, `.reduce` by `no-array-reduce`, `for...of` by the
  branch ban).
- **Codec** → DataView byte writer/reader (no bitwise, no loops); the 4-arg
  encoders (`encodeClientIntent`/`encodeWelcome`) take options objects (`max-params`).
- **Absent collaborators** → null-objects (`NULL_TRANSPORT`, the WebSocket
  `SocketSink`) instead of `?.`/`=== null` presence checks.

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

## Design signal: the async transport layer (resolved)

The browser transports (WebTransport stream reads, WebRTC negotiation) are built
on `async`/`await` + stream loops with no clean branchless form. **Resolution:**
they are the SDK's **platform edge** — `webtransport.ts`, `webrtc.ts`, and the
transport-construction wiring `build-transport.ts` are isolated files, scoped out
of the branch ban (plus the async/await + `no-unsafe-*` boundary rules) and out of
the coverage gate, verified via Playwright — exactly as the Rust spine scopes its
`host`/`windowing` layers out of `engine_no_branching`. The reusable spine (codec,
client state machine, the default WebSocket transport) stays fully branchless and
at 100% coverage. The exemptions are documented in `.oxlintrc.json` and
`STATIC_ANALYSIS.md`; nothing was disabled per-line.

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
