# SPEC-12 — Host bridge & persistence

> Status: Partial — native seam landed and native-tested; the TS contract path is **not yet bound to the live channel**.
> Landed (2026-06-28, native): `axiom-host` gained `HostSessionConfig`/`HostOutcome`/`HostOutcomeSet` (plus the `Score`/`Pixels` host quantity newtypes), and the Rust wasm arm's inbound decode + emit-once outcome latch exist and are native-tested; `@axiom/game` declares the `getSessionConfig`/`notifyReady`/`reportOutcome`/`reportOutcomes` projection. **Gap:** the TS↔wasm binding is **STUBBED** — `wasm-host.ts` `deferredBridge()` returns inert no-ops for `getSessionConfig` (empty `{seed: 0n, params: {}}`), `notifyReady`, `reportOutcome`, and `reportOutcomes`, and `notifyReady`/`reportOutcomes` have **no `WasmGame` export at all**. So the live `postMessage`/URL-param/`localStorage` channel is not yet wired to the engine's `HostOutcome`; the native seam works in native tests, but the author-facing path does not reach the live channel.
> Contract: §15, §16.6   Vocabulary: the embed seam (Outcome report, localStorage, fetch record-gameplay, URL-param config injection, external reward/webhook, postMessage capability bridge, JWT handshake)   Determinism: boundary

## 1. Summary

This is the **embed seam**: the seam between a game and the page/widget that
hosts it. Config flows **in** (a seed and opaque parameters), a terminal outcome
flows **out** (won/score/metrics), and nothing in between is part of the
contract. The contract makes it deliberately minimal — `getSessionConfig`,
`notifyReady`, `reportOutcome` (§15), plus `reportOutcomes` for per-player
results (§16.6).

The vocabulary marks **one** primitive universal: all 11 games already end with a
`postMessage` `"complete"` / `gameMetrics` report to the parent frame, and it
"maps directly onto the existing `resolve → outbox` path" (vocabulary §"One
primitive is universal"). This spec standardizes that one seam — the outcome
report — and the config injection that feeds the sim's seed. The rest of the
embed row (localStorage `n=7`, fetch record-gameplay `n=6`, URL-param injection
`n=5`, reward/webhook `n=3`, capability bridge `n=2`, JWT `n=2`) is non-sim
host plumbing that rides the same seam or is host policy, not engine surface.

## 2. Current state (verified)

- **The host *stepping* boundary exists; the *session* boundary does not.**
  `axiom-host` is the sanctioned platform-facing layer. Its `HostApi`
  facade owns viewport validation (`HostViewport`), explicit per-frame input
  (`HostFrameInput`, host supplies every timestamp), lifecycle
  (`HostLifecycleState`/`Signal`), the deterministic step planner/driver
  (`HostBoundaryConfig`, `HostStepPlan`, `HostStepDriver`), per-frame reports
  (`HostFrameReport`), and the browser-free presentation boundary
  (`HostPresentationTarget`/`SurfaceHandle`/`SurfaceDescriptor`/`AdapterRequest`/
  `DeviceRequest`/`PresentationRequest`/`PresentationReport`). It calls **no**
  browser/DOM/clock API — "every nondeterministic value enters as explicit data"
  (`lib.rs` header).
- **No session-config type.** Nothing in the tree carries a host-supplied `seed`
  + opaque `params` into the engine. `HostBoundaryConfig` is timing policy
  (fixed step, max catch-up steps), not session identity.
- **No outcome type.** There is no `Outcome`, no terminal won/score/metrics
  report, and no engine path that forwards one to a host channel.
- **No `window.postMessage` / parent-frame reporting anywhere.** Verified by
  source scan: zero `postMessage`/`gameMetrics` outcome emit in any crate,
  module, or app. Each hosted game hand-rolls its own `postMessage("complete")`
  outside the engine.
- **No persistence layer.** No `localStorage`, no best-score/mute/leaderboard
  read-back, nothing of the `n=7` persistence row.
- **No URL-param / query-string config parsing.** The `n=5` config-injection row
  is unbuilt as engine surface.
- **No fetch record-gameplay POST.** Recording is internal to `axiom-recording`
  (a deterministic, memory-bounded frame recorder over opaque bytes); it is
  **never sent** anywhere. The `n=6` "POST the result to a server" row does not
  exist.
- **Net result:** the embed seam is essentially unbuilt as an engine-standard
  surface, despite every game speaking its one universal word by hand.

## 3. Architectural placement

The seam splits into a **neutral data half** (in the spine) and a **platform
half** (outside it), exactly as SPEC-00 split the loop into accumulator + app.

1. **Neutral session seam — extend `axiom-host`.** Two new
   primitive-only, browser-free data contracts and the facade methods that mint
   and validate them:
   - **`HostSessionConfig`** — the validated *input*: a `seed` plus opaque
     key→value `params` (string or number). This is the data shape the platform
     half decodes a URL query string / `postMessage` payload / JWT claim into; the
     host layer only validates and carries it, it never parses a query string.
   - **`HostOutcome`** (single-player) and a per-player `HostOutcomeSet`
     (`PlayerId → HostOutcome`, §16.6) — the validated terminal *output*:
     `won`, `score`, optional named `metrics`. Minted once, carried as data to
     the platform half.

   This is legal and correct here under two laws:
   - **Layer Law.** `axiom-host` is the deterministic host *boundary*; "it knows
     that a host exists" and accepts host facts "as explicit data" (`layer.toml`).
     A session config (in) and an outcome (out) are precisely host facts —
     siblings of `HostFrameInput` and `HostFrameReport`. They `depends_on`
     kernel/runtime only (seed is a `u64`/kernel id; nothing higher).
   - **Module Law #9.** Browser/platform APIs are layer-`host`-only plus
     `windowing`. The *neutral* seam touches no browser API, so it is legal in
     `host`; the *binding* that does is confined to the platform arm below.

2. **Platform bindings — `apps/axiom-game-runtime` (SPEC-00) / the `wasm32`
   platform arm.** The code that actually calls `window.parent.postMessage`,
   reads `window.location.search`, touches `localStorage`, or issues a `fetch`
   POST is a browser API. It lives where the contract already puts platform
   bootstrap: the runtime *app* (the leaf composition + platform edge), compiled
   for `wasm32`. The runtime app: decodes the inbound host channel (URL params /
   parent message / JWT) into a `HostSessionConfig`, hands it to the engine
   before tick 0, and drains the engine's single `HostOutcome` back out to the
   parent via `postMessage`. **Never** in a portable module — a module that
   referenced `localStorage`/`postMessage` fails the Module-Law #9 hygiene scan,
   and that is correct: persistence and parent-frame messaging are not reusable
   engine capability, they are host wiring.
   - If a future binding genuinely belongs *inside* the engine (e.g. a host hook
     on `axiom-host` itself driving the channel), it is an explicit allowlist
     amendment in `crates/xtask/src/hygiene.rs`, never a default — same bar as
     `windowing`.

3. **TS host bridge — part of `@axiom/game` (SPEC-00).** `getSessionConfig`,
   `notifyReady`, `reportOutcome`, `reportOutcomes` are the author-facing
   projection, exported from the one `@axiom/game` SDK and held to the TS spine
   laws. They bind the runtime app's `#[wasm_bindgen]` exports; they do not
   address the host directly (§15: "the game does not address the host directly").

The seam made physical: **`HostSessionConfig` and `HostOutcome` are data the
spine names; the host channel that carries them is not.** The engine owns
delivery of the outcome to its host (§15) by handing exactly one validated
`HostOutcome` to the runtime app, which forwards it — once.

## 4. API surface

### 4.1 Native

`axiom-host` (extended, boundary-class — primitive-only, no browser API):

```rust
// Inbound: the validated session identity the platform arm decoded.
impl HostApi {
    // Mint a session config from a seed and already-decoded opaque params.
    pub fn session_config(
        &self,
        seed: u64,
        params: HostSessionParams,
    ) -> HostSessionConfig;
}

// Outbound: the terminal outcome, minted once.
impl HostApi {
    pub fn outcome(&self, won: bool, score: f64) -> HostOutcome;            // metrics empty
    pub fn outcome_with_metrics(
        &self,
        won: bool,
        score: f64,
        metrics: HostMetrics,
    ) -> HostOutcome;

    // Per-player terminal outcomes (§16.6); cross-ref SPEC-13.
    pub fn outcome_set(&self, entries: /* PlayerId -> HostOutcome */) -> HostOutcomeSet;
}
```

`HostSessionParams` / `HostMetrics` are opaque ordered key→value maps (stable
iteration order — a determinism requirement, §6). No method on `axiom-host`
reads a clock, a URL, or `localStorage`; those are the runtime app's job.

`apps/axiom-game-runtime` (wasm-bindgen, boundary): owns the actual host channel
— decode inbound (URL query / parent `postMessage` / JWT claim) → `session_config`
before tick 0; emit one outbound `postMessage` from the engine's single
`HostOutcome`. Not a reusable facade; outside the coverage gate; ships with slice
tests.

### 4.2 TS authoring projection (the contract, §15 / §16.6)

```ts
// Read host-supplied configuration (seed plus opaque string/number parameters).
function getSessionConfig(): { seed: bigint; params: Record<string, string | number> };

// Signal readiness once the first frame can render.
function notifyReady(): void;

// Emit the terminal outcome exactly once. The engine forwards it to the host channel.
interface Outcome { won: boolean; score: number; metrics?: Record<string, number> }
function reportOutcome(o: Outcome): void;

// Per-player outcomes for a room; the authority reports each result (§16.6).
function reportOutcomes(results: Record<PlayerId, Outcome>): void;
```

`reportOutcome` / `reportOutcomes` are **emit-exactly-once**: the runtime app
latches the first call and rejects (no-ops) any later one, so a game cannot
report two terminal states. `getSessionConfig` returns the same value for the
whole session — it is read before tick 0 and never changes.

## 5. Data contracts

- **`HostSessionConfig`** `{ seed: u64, params: HostSessionParams }` — the
  inbound seam. `seed` is the determinism input (§6); `params` are opaque
  string/number values the *game* interprets (uid, prize threshold, mode) — the
  engine never branches on them. Projected as
  `{ seed: bigint; params: Record<string, string|number> }`.
- **`HostOutcome`** `{ won: bool, score: f64, metrics: HostMetrics }` — the
  outbound seam, minted once. The single universal word the whole catalog already
  speaks; standardizing it is this spec's core deliverable.
- **`HostOutcomeSet`** `PlayerId → HostOutcome` — the multiplayer terminal seam
  (§16.6); the authority owns it. Cross-ref **SPEC-13** for `PlayerId` and the
  authority deployment.
- **`HostSessionParams` / `HostMetrics`** — opaque, **stably-ordered**
  key→value maps (the only cross-boundary shapes that need iteration; order is
  fixed so the projected record and any logged form are reproducible).

All five are primitive-only and contain no browser/DOM object — the same
discipline as every existing `axiom-host` boundary type.

## 6. Determinism

This is a **boundary** spec; it touches both sides and obeys each.

- **`seed` is a determinism INPUT and must be fixed before tick 0.** It comes
  from `getSessionConfig` and seeds the sim's `Rng` (SPEC-01). Therefore config
  injection — whatever channel decodes it (URL param, parent message, JWT claim)
  — must complete *before the first fixed update*. A seed (or any `params` the
  game reads into sim state) that arrived after tick 0 would change history
  mid-replay; the runtime app resolves the full `HostSessionConfig` up front and
  treats it as immutable for the session.
- **`reportOutcome` is an output SIDE-EFFECT, not sim state.** It is derived from
  sim state (final score, win flag) but emitting it changes nothing the sim reads
  back. It is presentation-side relative to the tick loop: emit-once, never fed
  to a step. A replay produces the *same* `HostOutcome` because it is a pure
  function of the (deterministic) final state — but the *act* of posting it is
  out of contract.
- **The host channel itself is out of contract** (§15: "the host channel is not
  part of this contract"). `postMessage` delivery, retries, JWT verification, and
  CORS are the host's concern; the engine guarantees only that it hands over
  exactly one validated `HostOutcome`.
- **No clock, no randomness in this layer.** `axiom-host` mints these types from
  explicit data, consistent with its existing "every nondeterministic value
  enters as explicit data" rule.

## 7. Acceptance / proof

- **`axiom-host` extension** (sim-discipline spine: branchless, 100% covered):
  - `HostSessionConfig` round-trips `seed` and `params` with stable param order;
    constructing it twice from equal inputs yields equal values.
  - `HostOutcome` / `HostOutcomeSet` carry `won`/`score`/`metrics`; metric order
    is stable; equal inputs ⇒ equal outcomes (the replay-equality property).
  - Curated-export test (`tests/architecture.rs::lib_exports_are_curated_set`)
    updated to include exactly the new public types — no accidental widening.
- **`@axiom/game` bridge** (tsgo + Oxlint branch-ban + 100% TS coverage):
  - `getSessionConfig` returns a fixed value across calls within a session.
  - `reportOutcome` emits once; a second call is a no-op (latched), asserted.
  - `reportOutcomes` maps each `PlayerId` to its `Outcome`.
- **Slice test in `apps/axiom-game-runtime`** (outside the gate; slice-tested):
  a trivial authored game reads a seed via `getSessionConfig`, advances by `rng`,
  and calls `reportOutcome` once — proving the whole seam end-to-end (this is the
  same end-to-end proof SPEC-00 §7 references for SPEC-12/§15). The actual
  `window.postMessage` emit is verified on the browser/Playwright path, since the
  `wasm32` channel binding is outside the native gate.

## 8. Dependencies & order

- **Depends on SPEC-00** (the `@axiom/game` SDK and `axiom-game-runtime` app must
  exist for the bridge to bind and the channel to be driven). The seam is the
  natural *first* capability after the loop: SPEC-00's own slice proof already
  reaches for `reportOutcome`.
- **Cross-refs SPEC-01** (`seed` feeds the seeded `Rng`) and **SPEC-13**
  (`PlayerId`, `reportOutcomes`, the authority that reports per-player results).
- **Nothing in the spine depends on this**; it is a leaf seam. Apps consume it.

## 9. Open questions

- **localStorage is non-authoritative client state — confirm it never feeds the
  sim.** Best-score / mute / leaderboard (`n=7`) are persistence the *page* owns,
  not the simulation. If a stored value were ever read into sim state it would
  break replay (the same state would diverge by what happened to be cached). The
  invariant: persistence is read/written only by the runtime app / page, never
  passed into a fixed update. Open: does the engine even need a typed persistence
  facade, or is `localStorage` purely the app's business? Lean app-side until a
  second consumer proves a primitive — same call SPEC-00 makes for handle tables.
- **Security of inbound config is host policy — out of scope but required.**
  URL-param injection, the `postMessage` capability bridge (`n=2`), and the
  origin-checked JWT handshake (`n=2`) all carry trust decisions: which origins
  may seed a session, whether a JWT is valid, whether a `params` value is
  attacker-controlled. The engine validates *shape* (`HostSessionConfig`), never
  *trust*. Flag explicitly: the runtime app / host must origin-check and verify
  before minting a config; this spec does not specify that policy.
- **`score` type.** Contract `Outcome.score` is a JS `number` (f64). Confirm f64
  is acceptable at the outbound boundary (it is non-sim, so float
  non-determinism across machines does not violate §17 — the outcome is reported,
  not replayed-into). If a game wants an exact integer score, it carries it in
  `metrics` or encodes it losslessly in the f64 range.
- **External reward / webhook (`n=3`).** A direct points / webhook POST is a
  second outbound channel beyond the parent `postMessage`. Is it just another
  runtime-app delivery of the same `HostOutcome`, or a distinct authenticated
  side-effect the host owns entirely? Default: it is the host's job — the engine
  emits one `HostOutcome`; whether the host turns that into a webhook is out of
  contract.
