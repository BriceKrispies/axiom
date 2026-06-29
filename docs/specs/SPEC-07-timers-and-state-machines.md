# SPEC-07 — Timers & state machines

> Status: Landed
> Landed (2026-06-28): kernel `TickSchedule` (the extracted `(Tick, Id)` wake queue) + new module `axiom-tick` (`TickApi`: `after`/`every`/`cancel`/`due`, `create_machine`/`transition`/`drain_events`); `@axiom/game` `Sim.time` (`makeTime`) + `BridgeStateMachine`, dispatched by the per-tick `TickPump`. The §2 gaps below are now closed.
> Contract: §9   Vocabulary: Timer / countdown / cooldown (claims have, actually missing), Game-flow state machine (partial), Per-entity state machine (partial)   Determinism: sim

## 1. Summary

Tick-scheduled control flow: "do this in 90 ticks," "every 30 ticks spawn a
wave," "while in `Stunned`, drift; on enter, flash." The contract gives the
author `Timers` (`after`/`every`/`cancel` → `TimerId`) and a generic
`StateMachine<S>` (`current`/`ticksInState`/`transition`, with
`onEnter`/`onUpdate`/`onExit`) — §9, the deterministic, **wall-clock-free**
half of "time & state." Both are `sim`: a cooldown, a spawn cadence, and a
round-phase clock all decide gameplay and must replay byte-identically and
reconcile across machines (§17).

The vocabulary inventory marks `Timer/countdown/cooldown` as **have (11/11)** —
an over-claim. All 11 games gate behavior on tick windows; six author a
game-flow machine (menu/playing/over), four a per-entity machine (poses, AI
modes, round phases). None of the three named primitives exists as a reusable
shape today (§2). Every game wants them; none should hand-roll them.

## 2. Current state (verified)

- **Timers (`after`/`every`/`cancel`): missing.** No engine layer or module
  exposes a named timer. The fixed-step tick counter exists —
  `axiom_kernel::Tick` (monotonic, saturating, clock-free), advanced by
  `axiom-runtime` and enveloped per frame by `axiom-frame` — so a game *can*
  gate manually (`if sim.tick == deadline`), but the contract primitive is
  absent. The vocabulary's "Timer = have (11/11)" counts the raw counter as the
  capability; it is not.
- **A different, deeper scheduler already exists in `axiom-sim-core`.** Its
  private `ProcessScheduler` (`scheduler.rs`) + `ProcessWakeQueue`
  (`process_wake_queue.rs`) are a deterministic wake queue keyed by `(SimTick,
  ProcessId)`: `schedule_wake`/`cancel`/`take_due`(pop due in `(tick, id)`
  order)/`finalize`(reschedule). **This is exactly the timer scheduling shape**
  — one pending wake per id, replace-on-reschedule, range-query for due, cancel
  removes — proven and tested. But it is `pub(crate)`, welded to sim-core's
  process/effect/causal-journal machinery (`HandlerSpec`, lifecycle, dependency
  subscriptions), and lives in an *isolated engine module* no other module may
  import (Module Law #2). It is the right primitive in the wrong shape, the
  wrong visibility, and an un-importable home. §3 resolves this without
  duplicating it.
- **General-purpose `StateMachine`: missing.** No game-flow or per-entity FSM
  type exists; apps hand-roll state with ad-hoc enums + a per-tick counter
  (vocabulary: "partial" for both rows). sim-core's `ProcessLifecycle` is a
  fixed process-status machine, not the generic, author-defined FSM §9 asks for.
- **TS surface: absent.** `packages/axiom-client` is a netcode client; no
  `@axiom/*` `Timers`/`StateMachine` projection exists (per SPEC-00).

## 3. Architectural placement

Two pieces at two tiers — the **mechanism** drops to the spine, the **facade**
is a module:

1. **Shared mechanism → extract sim-core's wake queue down into the kernel as a
   generic `TickSchedule`.** sim-core proves the tick-keyed wake queue is needed
   by a real consumer; Timers makes it two. Under the Module Law, when two
   modules need one primitive, **the primitive belongs in a lower layer, not a
   third module and never module→module**. The lowest correct home is the
   **kernel**: a `(Tick, Id)`-ordered schedule with replace-on-reschedule,
   `cancel`, and `pop_due` is *pure deterministic tick + identity data* — it
   reads no clock, owns no domain, no ECS, no process/effect meaning. That is a
   "deterministic time/tick primitive" over "stable IDs" — precisely the two
   things the kernel is *for*, and the textbook case of "a broadly-shared
   primitive no single adjacent layer owns belongs in the kernel" (CLAUDE.md).
   `sim-core`'s domain-specific `WakeReason` rides on top as the schedule's
   generic payload; `ProcessScheduler` re-bases onto the kernel primitive and
   loses its bespoke copy. **This is not a duplicate of sim-core and not a
   projection over it (illegal across modules) — it is the de-duplication: one
   tested schedule in the kernel, two consumers above it.**

2. **Facade → new engine module `axiom-tick`.** `Timers` and `StateMachine` are
   generic gameplay-flow capabilities with no domain meaning — an *isolated*
   engine module exposing **one** facade (`TickApi`) plus its id vocabulary,
   `allowed_modules = []`, `allowed_layers = ["kernel"]`. It builds `Timers`
   as a thin projection of the kernel `TickSchedule` (timer-id payloads, one-shot
   vs repeating), and `StateMachine` as a tiny `(current, entered_tick)` record
   with transition logic. It needs nothing above the kernel: the current `Tick`
   is *supplied* by the caller each step (exactly as sim-core's
   `SchedulerStep::new(tick)`), so the module never reads a clock and stays maximally
   isolated. It is **not** a layer: it is a gameplay capability, not spine
   infrastructure — the spine part is the kernel `TickSchedule`. `sim`-class,
   branchless, 100% covered.

The callbacks themselves do **not** live here — see §5/§9: the native core owns
the *schedule* (pure data); the author's closures live TS-side and are invoked
by the runtime app from the per-tick fired-id list, the same way SPEC-00 keeps
handle tables out of sim state.

## 4. API surface

### 4.1 Native

`axiom-kernel` (new, the extracted mechanism, `sim`-class):

```rust
// Deterministic (tick, id)-ordered schedule with at most one pending entry per id.
// Generic over an opaque id and a payload; reads no clock — the tick is supplied.
impl<Id: Ord + Copy, P: Copy> TickSchedule<Id, P> {
    pub fn schedule(&mut self, id: Id, at: Tick, payload: P);   // replaces any pending entry
    pub fn cancel(&mut self, id: Id) -> bool;                   // removes; clean false if absent
    pub fn pending(&self, id: Id) -> Option<Tick>;
    pub fn pop_due(&mut self, now: Tick) -> Vec<(Id, P)>;       // due at/before `now`, (tick,id) order
}
```

`axiom-tick` (new module facade, `sim`-class). `Timers` projects `TickSchedule`;
`StateMachine` is the small record. The current `Tick` is passed in per step.

```rust
impl TickApi {
    // Timers — payload is (interval, one-shot|repeating); ids are ascending TimerId.
    pub fn after(&mut self, now: Tick, ticks: TickDelta) -> TimerId;   // one-shot
    pub fn every(&mut self, now: Tick, ticks: TickDelta) -> TimerId;   // repeating (interval >= 1)
    pub fn cancel(&mut self, timer: TimerId) -> bool;
    pub fn due(&mut self, now: Tick) -> Vec<TimerId>;   // fired this tick, (tick,id) order;
                                                        // repeating timers self-reschedule here

    // State machines — author state is a state index + entered tick (pure data).
    pub fn create_machine(&mut self, states: u32, initial: u32, now: Tick) -> StateMachineId;
    pub fn transition(&mut self, m: StateMachineId, to: u32, now: Tick);
    pub fn current(&self, m: StateMachineId) -> Option<u32>;
    pub fn ticks_in_state(&self, m: StateMachineId, now: Tick) -> Option<TickDelta>;
    pub fn drain_events(&mut self, now: Tick) -> Vec<StateEvent>;   // Enter/Update/Exit, deterministic order
}
```

`due` and `drain_events` return **data** (fired ids, state transitions); the
runtime app turns that data into `onEnter`/`onUpdate`/`onExit`/timer-callback
invocations TS-side. No closure is stored in `sim` state.

### 4.2 TS authoring projection (the contract, §9)

```ts
type TimerId = Handle;

interface Timers {
  after(ticks: Ticks, cb: () => void): TimerId;     // one-shot
  every(ticks: Ticks, cb: () => void): TimerId;     // repeating
  cancel(id: TimerId): void;
}

interface StateMachine<S extends string> {
  readonly current: S;
  readonly ticksInState: Ticks;
  transition(to: S): void;
}
interface StateDef<S extends string> {
  onEnter?: (sm: StateMachine<S>) => void;
  onUpdate?: (sm: StateMachine<S>) => void;          // each tick while active
  onExit?: (sm: StateMachine<S>) => void;
}
function createStateMachine<S extends string>(
  states: Record<S, StateDef<S>>, initial: S): StateMachine<S>;
```

`createStateMachine` maps the author's string states to dense indices by
declaration order (deterministic); `Sim.timers`/the machine handle are minted by
the runtime app and reach back into the native `TickApi`. The closures stay in
the TS layer and are re-bound on replay (like handles, SPEC-00 §9).

## 5. Data contracts

- **`TimerId` / `StateMachineId`** — opaque `Handle` newtypes, allocated
  ascending; ascending order is the deterministic tie-break for entries due on
  the same tick. Never serialized into sim state semantics; a replay re-mints
  them.
- **The schedule** (`TickSchedule` entries: `(tick, id) → (interval, repeating)`)
  and **each machine's `(current_index, entered_tick)`** are the *only* sim
  state this spec owns. Both are pure data → snapshot/restore and delta
  replication are trivial (§16.5, SPEC-13) with no opaque code in the store.
- **Fired-this-tick outputs** — `Vec<TimerId>` and `Vec<StateEvent>` — cross
  the boundary to the runtime app each tick, ordered `(tick, id)`. They are
  derived per-tick values, not stored state.
- **Callbacks are not a data contract.** A closure is opaque code, not
  serializable bytes; it cannot live in a snapshot or on the wire. It stays
  TS-side (§9).

## 6. Determinism

- **Single clock (§17.1).** Every deadline is a `Tick`; `after(n)` schedules at
  `now + n`, `every(n)` reschedules `+n` on each fire. No wall-clock or
  frame-delta reaches the schedule — `now` is the supplied sim tick, never a
  platform time. This is the whole point of §9's "no wall-clock timers."
- **Stable simultaneous order (§17.4).** Due entries are keyed `(Tick, Id)` in a
  `BTreeMap`; `pop_due`/`due` return ascending — two timers due on the same tick
  fire in ascending `TimerId` (allocation order), identically every run and
  across machines. Same for `StateEvent` ordering by `StateMachineId`.
- **Branchless dispatch (Branchless Law).** Mirrors sim-core's proven style:
  due-timer dispatch is `pop_due(now).into_iter().map(...)`; one-shot vs
  repeating is table/`then` selection on the payload
  (`repeating.then(|| schedule(id, now + interval, payload))`), never `if`/
  `match`. An FSM step is `changed.then(|| (exit, enter)).unwrap_or(update)`
  over a state-index table — no `if`/`match`/`while`. The kernel `TickSchedule`
  uses the same `range(..=upper)` + `for_each` removal sim-core already ships.
- **Reentrancy is a boundary, not a branch.** A callback that schedules/cancels
  during a tick's dispatch mutates the schedule for **the next** tick, never the
  in-flight due pass — `due`/`drain_events` snapshot the due set first, exactly
  as sim-core stashes pending output and applies it at an explicit boundary.
  This keeps dispatch one immutable, order-stable, branchless pass.
- **Cross-instance (§17.6).** The schedule is integer-keyed `BTreeMap` ops only;
  authority and predicted clients fire the same `TimerId`s on the same ticks, so
  prediction reconciles without drift. Presentation never feeds a timer (§17.5).

## 7. Acceptance / proof

- **100% covered, branchless** across the kernel `TickSchedule` and the
  `axiom-tick` facade — the binding gate for `sim`-class spine code.
- **sim-core re-base is behavior-preserving.** After `ProcessScheduler`/
  `ProcessWakeQueue` re-base onto the kernel `TickSchedule`, **every existing
  sim-core scheduler test still passes unchanged** — the proof the extraction
  de-duplicates rather than forks behavior. sim-core loses its private
  `process_wake_queue.rs` mechanism (keeps only `WakeReason` as payload).
- **Timer replay/golden (sim).** A game schedules `after(3)`, `every(5)`,
  cancels one mid-flight; the per-tick fired-`TimerId` sequence over N ticks is
  golden and **byte-identical on a second run**. Tie-break golden: two timers
  due on the same tick fire ascending-id. `every` self-reschedule golden;
  `cancel` of an already-fired or unknown id is a clean no-op (`false`).
- **State-machine golden (sim).** `transition` emits exactly one `Exit(old)` +
  one `Enter(new)` on the transition tick and `Update(current)` on every other
  tick; `ticks_in_state == now - entered_tick`. The event sequence over N ticks
  reproduces on replay. Self-transition policy (§9) is tested explicitly.
- **Reentrancy test.** A timer whose dispatch schedules another timer: the new
  timer is due no earlier than `now + 1`, never within the same due pass — and
  this holds deterministically on replay.
- **Projection.** `@axiom/game` `Timers` + `StateMachine`: tsgo + Oxlint (branch
  ban) + 100% TS coverage. A headless authored game registers timers and a
  machine, runs N ticks, and asserts the callback fire order + state-callback
  order reproduce on a second run (the §17.4 ordering contract).

## 8. Dependencies & order

- **Build order: contract step 7** (after grid/pathfinding, SPEC-06), but only
  *soft*-ordered: the facade needs only the kernel `Tick` + `TickSchedule` and
  SPEC-00's per-tick `Sim`/runtime boundary that supplies `now` and holds the
  closures. SPEC-01's `Rng` is an *optional* input (jittered cadences) — not a
  prerequisite.
- **Prerequisite work (flag scope before landing):** extracting `TickSchedule`
  into the kernel and re-basing `axiom-sim-core` onto it is a kernel change that
  touches a module — far-reaching and outward-facing. Per No-Shortcuts, confirm
  scope first; it is the correct, non-duplicating path, but it must land *with*
  sim-core's tests green, not as a separate "later."
- **Downstream:** UI/HUD menus, pause, and result screens compose
  `StateMachine` (SPEC-09 §14 explicitly defers screen state to §9); round-phase
  and AI-mode logic across the sim consume both; netcode (SPEC-13) replicates
  the schedule + machine state as ordinary `sim` data (no special-casing,
  because no closures are in the store).

## 9. Open questions

- **Callbacks vs. data — the load-bearing one.** A closure cannot be
  snapshotted (§16.5) or replicated (SPEC-13). This spec resolves it by keeping
  the **schedule as data** in `sim` and the **closures in TS** (re-bound on
  replay, like handles). The sharper alternative is to make a timer *fire an
  author-defined event/intent* (a data tag the sim consumes) rather than invoke
  a closure at all — strictly more data-pure and netcode-friendly, at the cost
  of heavier authoring. Recommendation: ship the closure-in-TS form (it matches
  the contract's signatures) **and** confirm that a fired `TimerId`/`StateEvent`
  carrying an optional author event id is a clean superset, so a game that needs
  pure-data timers (deterministic server-only rooms) can opt in without a second
  API.
- **Is the schedule even replicated state, or derived?** If timers are only ever
  registered from deterministic sim code over the same input stream, the
  schedule *recomputes* identically on every instance and needs to ride the
  snapshot only for restore-without-replay, not the per-tick delta. Decide
  whether SPEC-13 treats it as authoritative state or derived — leaning derived,
  snapshot-on-restore only.
- **Kernel growth.** Is `TickSchedule` a kernel primitive, or does it warrant a
  dedicated low *scheduling* layer between kernel and the modules? Kernel is the
  argued home (pure tick+id data, broadly shared), but adding a generic
  collection to the "small, boring" kernel is exactly the kind of decision to
  confirm rather than assume.
- **StateMachine state home.** Is a machine a handle-managed native value
  (`StateMachineId` table in the runtime app) or an ECS component the author
  attaches (SPEC-02)? The component form makes hierarchy, query, and replication
  fall out for free; the handle form is simpler for a single game-flow machine.
  Likely both, with the component form canonical.
- **Degenerate intervals.** `every(0)` would busy-fire forever within a tick;
  clamp the repeat interval to `>= 1`. `after(0)` fires at the end of the
  current tick (next `due`), not retroactively. Confirm these are the contract's
  intended semantics so the boundary stays branchless and total.
- **Self-transition.** Does `transition(current)` re-emit `Exit`+`Enter` (reset
  `ticksInState`) or no-op? Default: re-emit and reset (a fresh `Enter` is the
  useful behavior for re-triggering a pose), but it must be one documented rule,
  not per-call ambiguity.
