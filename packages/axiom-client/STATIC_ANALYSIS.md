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

The whole static-analysis stack is native (Rust linter + Go type engine) — there
is no JavaScript `typescript` install.

## Running the gates

```sh
npm install              # once: resolves tsgo + oxlint + tsgolint + plugin
npm run typecheck        # tsgo --noEmit (strict tsconfig)
npm run lint             # oxlint --type-aware --deny-warnings
npm run coverage         # node:test, fails under 100%
npm run gate             # all three in sequence (typecheck → lint → coverage)
```

Or from the repo root: `make ts-gate` (runs `scripts/ts-gate.sh`).

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
  rule off for `test/**`), exactly as the Rust Branchless Law exempts tests.
- **Coverage** — `node --test --experimental-test-coverage` with
  `--test-coverage-lines/branches/functions=100` (test files excluded). Below
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
them in `.oxlintrc.json`, and they are **coverage-exempt** (excluded in the
`coverage` script), verified instead via the Playwright browser path. Everything
else stays fully branchless and at 100% node coverage.

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
ban) passes, and `node:test` coverage is **100%** lines/branches/functions across
every spine file. The gate is wired into the pre-commit hook and CI. The history
of the remediation is in [`../../docs/ts-sdk-hardening.md`](../../docs/ts-sdk-hardening.md).
