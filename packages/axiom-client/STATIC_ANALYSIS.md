# `@axiom/client` — static analysis & gates

This package (the Axiom multiplayer browser SDK) is held to TypeScript-native
versions of the Rust engine spine's laws. The Rust spine is gated by clippy +
dylint (static analysis), `engine_no_branching` (the Branchless Law), and
`cargo-llvm-cov` (the 100% Coverage Law). The TS SDK mirrors all three with a
fully native toolchain.

## The toolchain

| Engine (Rust) law / tool          | TypeScript SDK counterpart                                   |
|-----------------------------------|--------------------------------------------------------------|
| `cargo` / `rustc`                 | **`tsgo`** — TypeScript 7.0 native (Go) compiler (`@typescript/native-preview`) |
| clippy + dylint (`-D warnings`)   | **Oxlint** (Rust-native) — every category set to `error`, `maxWarnings: 0` |
| `engine_no_branching` dylint      | **`no-restricted-syntax`** branch ban (via `oxlint-plugin-eslint`) |
| typed lints (real type info)      | **Oxlint type-aware** via `tsgolint` (typescript-go, TS 7)   |
| `cargo-llvm-cov` 100% gate        | **`node:test`** built-in coverage at 100% lines/branches/functions |
| (the coverage law's structural floor) | **co-location gate** — every `src/<name>.ts` must have a sibling `src/<name>.test.ts` |

The whole static-analysis stack is native (Rust linter + Go type engine) — there
is no JavaScript `typescript` install.

## Running the gates

```sh
npm install              # once: resolves tsgo + oxlint + tsgolint + plugin
npm run typecheck        # tsgo --noEmit (strict tsconfig)
npm run lint             # oxlint --type-aware --deny-warnings
npm run colocation       # every src file has a sibling *.test.ts (../../scripts/ts-colocation-check.mjs)
npm run coverage         # node:test, fails under 100% (../../scripts/ts-coverage.mjs)
npm run gate             # all four in sequence (typecheck → lint → colocation → coverage)
```

Or from the repo root: `make ts-gate` (runs `scripts/ts-gate.sh` over **both**
TS packages — `@axiom/client` and `@axiom/game` — with the same four-stage gate).

## What each gate enforces

- **Type check (`tsgo`)** — the existing strict `tsconfig.json` (`strict`,
  `noUncheckedIndexedAccess`, `verbatimModuleSyntax`, …) compiled by the
  TypeScript 7.0 native compiler. `@types/node` is a declared devDependency
  because the SDK genuinely uses Node globals (`setInterval`, `node:test`).
- **Lint (Oxlint)** — `.oxlintrc.json` sets `correctness`, `suspicious`,
  `pedantic`, `perf`, `style`, `restriction`, and `nursery` all to `error`, with
  `maxWarnings: 0` and `--deny-warnings`, so **any** finding fails the run.
  Type-aware rules (`no-floating-promises`, `switch-exhaustiveness-check`,
  `strict-boolean-expressions`, `prefer-readonly-parameter-types`, the
  `no-unsafe-*` family, …) run through `tsgolint`.
- **Branch ban** — `eslint-js/no-restricted-syntax` bans `if`, ternary `?:`,
  `switch`, `for`/`for...in`/`for...of`, `while`/`do...while`, `&&`/`||`/`??`,
  and optional chaining `?.`. Tests are exempt (an `overrides` block turns the
  rule off for `**/*.test.ts` and `**/*.testkit.ts`), exactly as the Rust
  Branchless Law exempts tests.
- **Co-location** — `scripts/ts-colocation-check.mjs` fails unless **every**
  `src/<name>.ts` has a sibling `src/<name>.test.ts`. This is the structural
  floor under the coverage law: `node --test` coverage only reports files that a
  test actually *imports*, so a file no test touches is silently invisible (not
  failing). Forcing a co-located test per source file pulls every file into the
  report, so the 100% gate genuinely bites. Test-tier files (`*.test.ts`,
  `*.testkit.ts`) and the platform-edge files listed in `test-exempt.json` are
  exempt; the checker also fails on a stale exempt entry or an exempt file that
  *does* have a test (keeping the exemption list honest).
- **Coverage** — `scripts/ts-coverage.mjs` runs `node --test
  --experimental-test-coverage` with `--test-coverage-lines/branches/functions=100`.
  Test-tier files are excluded, and the platform-edge exclusions are read from the
  **same** `test-exempt.json` the co-location gate uses — one list, so a file can
  never be dropped from coverage without being declared exempt in the open. Below
  100% on any metric exits non-zero.

## Module layout

The codec spine is split one-class-per-file (the lint forbids multiple classes
per file): `messages.ts` (constants + decoded shapes), `byte-writer.ts` /
`byte-reader.ts` (DataView-based, no bitwise), `protocol-error.ts` (the
`assert`/`fail` branchless validation primitives), `codec.ts` (encoders/decoders +
total-`Record` dispatch), `branchless.ts` (`pick`/`coalesce`/`each` selection
primitives), `text.ts`, `transport.ts` (the WebSocket transport + `asUint8Array`),
`client-config.ts`, and `client.ts` (the branchless state machine). `index.ts` is
the public facade.

## The platform edge

`webtransport.ts`, `webrtc.ts`, and `build-transport.ts` bind browser-only APIs
(`WebTransport`, `RTCPeerConnection`, `fetch`, `new WebSocket`) whose async/stream
control flow has no clean branchless form. Like the Rust spine's `host`/`windowing`
layers, they are the **platform edge**: a documented subset of rules (the branch
ban, `no-async-await`, `await-in-loop`, the `no-unsafe-*` family) is scoped off for
them in `.oxlintrc.json`, and they are **coverage- and co-location-exempt**
(declared once in `test-exempt.json`, which both the coverage runner and the
co-location gate read), verified instead via the Playwright browser path.
Everything else stays fully branchless, at 100% node coverage, and carries a
co-located unit test.

## Documented exceptions

These are the only relaxations, each justified in `.oxlintrc.json`:

- **`prefer-readonly-parameter-types` is off globally** — a TypeScript
  type-system impossibility, not a code shape: a binary codec is built on
  `Uint8Array`/`DataView`, which have no readonly form (`Readonly<Uint8Array>`
  still exposes `set`/`fill`), and `readonly number[]` would destroy the binary
  contract and performance.
- **The platform-edge subset** (above), scoped to two/three files.
- **Tests** keep the correctness/suspicious/perf/type-aware rules but relax the
  production-ergonomics pedantic/style/restriction rules (byte-vector literals,
  short conventional names, fixture array access), matching how clippy treats
  Rust tests.

## Status

**Green.** `tsgo` typecheck passes, Oxlint (every category an error + the branch
ban) passes, the co-location gate passes (every `src/<name>.ts` has a sibling
`src/<name>.test.ts`), and `node:test` coverage is **100%** lines/branches/functions
across every spine file — for **both** `@axiom/client` and `@axiom/game`. Tests are
co-located beside the source they exercise (one `*.test.ts` per file), with shared
fixtures as `*.testkit.ts`. The four-stage gate is wired into the pre-commit hook
and CI. The history of the remediation is in
[`../../docs/ts-sdk-hardening.md`](../../docs/ts-sdk-hardening.md).
