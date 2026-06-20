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

## Status

**Phase 1 (tooling) is in place; the SDK is not yet green.** Turning these gates
on at full strength surfaces the existing backlog (branchy control flow, typed-lint
findings, and sub-100% coverage). The remediation — a branchless rewrite of
`client.ts`/`protocol.ts`/`transport.ts` and the drive to 100% coverage — is
tracked in [`../../docs/ts-sdk-hardening.md`](../../docs/ts-sdk-hardening.md).
Until it lands, the gate is intentionally **not** wired into the blocking
pre-commit hook or CI, mirroring how the Rust spine ran its unbranching loop
before `engine_no_branching` went hard.
