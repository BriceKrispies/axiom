# Arena Forge — Testing

How to run Arena Forge's tests and headless harness, what the harness proves,
and how the test suite's invariants map to the game's structural guarantees.

## Running the tests

```sh
cd apps/arena-forge/web
node --test "src/**/*.test.ts"
```

This repo's Node (v24) type-strips `.ts` natively — there is no build step,
no `ts-node`, no transpile-then-run. The sim/api/bots/harness test files
import no DOM globals and never import `@axiom/web-engine`, so they run
exactly as-is under bare `node --test` (this is *why* the simulation package
is required to stay DOM-free and renderer-free — see `ARCHITECTURE.md`).

Current suite (verified green):

```text
✔ same seed + command stream produces byte-identical final state and events
✔ different seeds produce controlled variation (not all identical winners)
✔ all 100 seeded matches complete, legally, within the round cap, with no negative gold
✔ every group is used across the suite (archetypes are all reachable)
✔ content bundle passes validation
✔ exactly 36 collectible cards
✔ exactly 4 groups
✔ each group has exactly 8 collectible members
✔ exactly 4 neutral collectible cards (belonging to no group)
ℹ tests 9  pass 9  fail 0
```

Two test files today, both co-located next to the code they exercise (the
established pattern for future test files to follow — a `*.test.ts` sibling
of its source, not a separate `tests/` tree):

- `src/harness/headless.test.ts` — determinism + full-suite invariants (uses
  the harness described below).
- `src/sim/content/content.test.ts` — validates the authored content bundle
  itself (shape and `validateContent` cleanliness), independent of match
  play.

## Type checking

`tsgo` (the native TypeScript-Go compiler used across this repo's TS
packages) is vendored under `packages/axiom-game`'s `node_modules`. Arena
Forge's `web/` does not yet have its own `package.json`/`node_modules` (the
browser-facing shell hasn't been built — see `ARCHITECTURE.md`), so invoke
the vendored binary directly against Arena Forge's own `tsconfig.json`:

```sh
cd apps/arena-forge/web
../../../packages/axiom-game/node_modules/.bin/tsgo -p tsconfig.json --noEmit
```

(On Windows use `tsgo.cmd`.) This passes clean today under `strict`,
`exactOptionalPropertyTypes`, and `noUncheckedIndexedAccess` — the same
strictness bar as `@axiom/client` and `@axiom/web-engine`. `tsconfig.json`
excludes `*.test.ts` from this compile (tests run under `node --test`'s own
type-stripping and never import `@axiom/web-engine`, so they need no
project-wide compile); the app's own `@axiom/web-engine` path mapping
resolves to `packages/axiom-web-engine/dist/index.d.ts` for whenever the
presentation/rendering layer starts consuming it.

Arena Forge is **not** currently wired into the repo-wide `ts-gate.sh` /
`Makefile` `ts-gate` target the way `@axiom/client` and `@axiom/web-engine`
are — there is no `apps/arena-forge/web/package.json` yet, and no Oxlint/
branch-ban/coverage gate runs against it today. The commands above are the
honest, currently-available way to verify it; wiring it into the shared
TS-spine gate is future work once the app has its own `package.json`.

## The headless harness

`src/harness/headless.ts` is the DOM-free simulation harness — it plays full
matches with eight bots (`LocalMatchHost` constructed with `allBots: true`,
so even the "human" slot is bot-driven) and derives a report entirely from
the authoritative event log and phase-transition log.

```ts
import { loadDefaultContent } from "../sim/content/bundle.ts";
import { runSeededMatch, runManyMatches, runDefaultSuite } from "./headless.ts";

const content = loadDefaultContent();

// One match, one seed:
const { host, report } = runSeededMatch(content, 4242);

// A batch: seeds baseSeed, baseSeed+1, … baseSeed+count-1:
const suite = runManyMatches(content, 100, 1);

// Convenience: the default 100-match suite on the default content, seed 1:
const suite2 = runDefaultSuite();          // runDefaultSuite(count = 100, baseSeed = 1, rules?)
```

`MatchReport` (per match) and `SuiteReport` (aggregated) fields, all derived
by scanning the match's event log and transition log — never by re-deriving
independently, so the report is only as trustworthy as the log itself (which
is exactly what the determinism test cross-checks):

| Field | Derived from |
|---|---|
| `complete` | `host.isComplete()` — phase reached `match_complete` |
| `rounds` | Final `state.round` |
| `winner` / `placements` | Players with `placement > 0`, sorted |
| `forgedUnits` | Count of `unit_forged` events |
| `boundCombats` | Count of `diagnostic` events (a combat force-ended by a runtime bound) |
| `illegalTransitions` | Count of logged `PhaseTransition`s that fail `isLegalTransition` (`phase.ts`) |
| `avgEliminationRound` | Average `round` at the time of each `player_eliminated` event |
| `groupUsage` | Per-group count of `card_purchased` events for cards in that group |
| `negativeGold` | Whether any player's final `gold` is negative |

## What the harness + tests prove

Directly mapped to the assertions in `headless.test.ts`:

1. **Determinism.** Two `LocalMatchHost`s built from the identical seed
   (`4242`) and run to completion produce byte-identical `JSON.stringify(
   { state, events })` — the entire final `MatchState` *and* the entire
   ordered event log match exactly. This is the strongest possible proof of
   the "same seed + command stream → same result" guarantee, since bots are
   deterministic policies driven purely by the seeded `Rng` and match state.
2. **Controlled variation.** 12 different seeds (1–12) produce at least 2
   distinct winning player ids — the sim isn't secretly deterministic
   *regardless* of seed (a degenerate always-same-winner outcome would
   indicate a broken or unused seed).
3. **100 matches all complete, legally, boundedly, with sane economy.** The
   standard 100-seed suite (`baseSeed = 1`) asserts, in one pass:
   - `allComplete` — every one of the 100 matches reaches `match_complete`
     (no hang, no stuck phase).
   - `illegalTransitions === 0` — every phase change across all 100 matches
     is a legal edge per `LEGAL_TRANSITIONS` (`phase.ts`); the phase machine
     table is checked as an independent spec, not just trusted.
   - `negativeGoldMatches === 0` — gold never goes negative in any match (the
     economy's `Math.max(0, …)`/full-validation-before-mutation discipline
     holds under 100 varied bot-driven games).
   - `maxRoundsSeen <= content.version + 60` — no match runs away past the
     round cap (a generous check against `DEFAULT_RULES.maxRounds = 60`).
   - `avgRounds > 1` — matches aren't trivially ending round 1 (a sanity
     floor, not a tight bound).
   - (Reported, not asserted: `avgRounds`, `avgEliminationRound`,
     `forgedTotal`, `boundCombats`, `winnerDistribution`, `groupUsage` — a
     human-readable shape check via `console.log`, e.g. the observed run
     shows `avgRounds≈13`, `forgedTotal=1514`, `boundCombats=0` across the
     standard 100-seed suite.)
4. **Every group is reachable.** A separate 30-match suite (seed 500) asserts
   every one of the four groups (`ironbound`, `emberkin`, `bloomtide`,
   `echowisp`) is purchased at least once somewhere across the suite — the
   bot policies (`bots/policies.ts`) aren't structurally starving an entire
   archetype out of play.
5. **Content integrity**, independent of match play (`content.test.ts`):
   the authored `ContentBundle` passes `validateContent` with zero errors,
   and the roster shape matches spec — exactly 36 collectible cards, exactly
   4 groups, exactly 8 collectible members per group, exactly 4 groupless
   neutrals.

Together these are the mechanical proof behind the claims in
`ARCHITECTURE.md`'s determinism section and `GAME_RULES.md` §9's termination
guarantee — not just prose asserting them.

## Where browser interaction tests would live

There are no browser/DOM interaction tests today (`src/presentation/`,
`src/audio/`, `src/ui/` are empty — see `ARCHITECTURE.md`). Once the
presentation shell exists, its own tests would follow the same co-located
`*.test.ts` convention as the sim tests, plus — for actually verifying
rendering in a real browser — the repo's Playwright controller
(`uv run scripts/playwright_controller.py …`, documented in the root
`CLAUDE.md`) driven against the app once it is served locally (e.g. via
`cargo run -p axiom-serve -- arena-forge`, once `axiom-serve` recognizes it
as a pure-TS `@axiom/web-engine` app the way it does `axiom-heat-check`).
