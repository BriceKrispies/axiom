# SPEC-05 — Input (keyboard, bindings, pointer, timing)

> Status: Draft
> Contract: §8   Vocabulary: Keyboard, Pointer/click, Touch/swipe/gesture, Key→action bindings, Charge/hold-release, Buffered direction, Timing-window hit   Determinism: sim

## 1. Summary

Every one of the 11 games needs to read input, and the simulation must read it
*deterministically*. The contract's rule (§17.3) is the whole point: raw device
events are **sampled into a per-tick intent snapshot before the sim sees them**,
so gameplay reads only author-defined action names against a fixed tick — never a
physical key, never a wall-clock event. This spec closes the gap between that
contract (§8) and a tree that today synthesizes only touch move/look + swipe and
keeps its one keybinding primitive in the wrong layer. It owns the *full* input
contract: keyboard state + edge detection, action bindings, pointer/click, the
existing swipe, and `pressedAtTick` for rhythm/reaction judging.

## 2. Current state (verified)

- **`axiom-input` exposes ONE facade, `TouchControls`** (`modules/axiom-input`,
  `allowed_layers = [kernel, math]`). It synthesizes from a frame of neutral
  `(Vec2, bool)` pointer samples: `update(surface, pointers) -> ControlFrame`
  (`move_vector`, `yaw`, `pitch` — analog first-person intent) and
  `swipe(surface, pointers) -> Option<Vec2>` (one unit direction on lift).
- **No keyboard primitive** in the module — no key state-map, no held set.
- **No edge detection** — nothing produces `pressed`/`released`; `TouchControls`
  reports per-frame deltas, not down/up transitions.
- **No action-binding facade.** A `Keymap`/`KeyBinding` does exist, but in
  `crates/axiom-interface` (`keymap.rs`) — the **UI layer**. `Keymap::resolve(key,
  event) -> Option<u32>` is a first-match key→`u32` lookup whose guard semantics
  (`routes_global_hotkey`, `in_text_field`, `console_focus`) are UI-chrome
  concerns, not sim input.
- **Pointer is move/look intent only** — no click/press events, no pointer
  position as a world point.
- **Missing entirely:** charge/hold-release, buffered direction, tick-stamped
  press (`pressedAtTick`) for timing windows.
- **TS `Input` interface does not exist** (no authoring SDK yet — see SPEC-00).

## 3. Architectural placement

**Extend the `axiom-input` engine module** to own the full input contract; do
**not** add a new layer or module. Justification under the Module Law:

- Input is one isolated capability with one facade — exactly an *engine module*.
  Its dependencies stay `[kernel, math]`; it composes no other module
  (`allowed_modules = []`) and touches no browser API (Module Law #9). The
  platform edge that captures raw `KeyboardEvent`/`PointerEvent`s stays in the
  `windowing` module / host; this module only folds **neutral samples** into the
  tick snapshot — the same boundary `TouchControls` already honors.
- **The action-binding primitive belongs here, not in `axiom-interface`.** The
  binding→action mapping the *simulation* reads is sim-class input, and
  `axiom-interface` is the UI layer: depending up into it is illegal, and even if
  it weren't, putting sim input behind a UI facade is the wrong home (No-Shortcuts
  — fix at the correct layer). `axiom-interface`'s `Keymap` stays for what it is —
  UI hotkey routing with text-field/console guards — and `axiom-input` owns its
  own guard-free binding table. They share a shape, not a crate (see §9).
- **Module Law #8 (one facade).** The module keeps exactly one public behavioral
  facade. Today that is `TouchControls`; this spec replaces it with `InputState`
  — the per-tick intent snapshot the sim reads — and folds touch/swipe synthesis
  and the binding table *behind* it. The pure id newtype `ActionId` is the
  sanctioned `pub use ids::{…}` vocabulary the facade traffics in (#8's id
  exemption — callers must be able to name the actions they bind and query).

## 4. API surface

### 4.1 Native (`axiom-input`, sim-class)

```rust
// The author-defined action name, interned to a stable id at bind time.
pub struct ActionId(/* opaque */);

// One facade: a tick-indexed intent snapshot + the binding table that built it.
impl InputState {
    pub fn new() -> Self;
    // Configure (and remap) which neutral key/button/gesture tokens fire an action.
    pub fn bind_action(&mut self, action: ActionId, keys: &[KeyToken]);

    // The sampling boundary (§6): fold one frame's worth of neutral device events
    // (key down/up set, pointer samples, surface) into THIS tick's snapshot,
    // resolving bindings and computing edges against the previous tick.
    pub fn sample(&mut self, tick: Tick, frame: &DeviceFrame);

    // Per-tick reads the sim uses — all resolved against the snapshot for `tick`.
    pub fn is_down(&self, action: ActionId) -> bool;     // held this tick
    pub fn pressed(&self, action: ActionId) -> bool;     // down-edge this tick (no auto-repeat)
    pub fn released(&self, action: ActionId) -> bool;    // up-edge this tick
    pub fn axis(&self, neg: ActionId, pos: ActionId) -> i8;   // -1 | 0 | 1
    pub fn pointer(&self) -> Option<Pointer>;            // { pos: Vec2, down: bool }
    pub fn pointer_pressed(&self) -> Option<Vec2>;       // press position this tick
    pub fn swipe(&self) -> Option<SwipeDir>;             // completed gesture this tick
    pub fn pressed_at_tick(&self, action: ActionId) -> Option<Tick>;  // most recent down-edge
}
```

`ControlFrame` (analog move/look) survives as an internal synthesizer feeding the
pointer/swipe arms of `sample`; its first-person camera intent is consumed through
the same facade (an app maps it to its controller, as today).

### 4.2 TS authoring projection (the contract, §8)

```ts
type Action = string;

interface Input {
  isDown(action: Action): boolean;
  pressed(action: Action): boolean;
  released(action: Action): boolean;
  axis(neg: Action, pos: Action): -1 | 0 | 1;
  pointer(): { pos: Vec2; down: boolean } | null;
  pointerPressed(): Result<Vec2>;
  swipe(): "up" | "down" | "left" | "right" | null;
  pressedAtTick(action: Action): Result<Ticks>;
}

function bindAction(action: Action, keys: string[]): void;   // configured once, remappable
```

`Sim.input` (SPEC-00 §4.2) is an `Input` over the snapshot for the running tick.
Gameplay reads **only** action names; physical keys appear only in `bindAction`.

## 5. Data contracts

- **`DeviceFrame`** — the neutral, recordable event bundle crossing the platform
  edge into `sample`: the set of key/button `KeyToken`s down this frame and the
  pointer `(Vec2, bool)` samples. No `web_sys` types; tokens are layout-stable
  strings (`KeyboardEvent.code`-style) decoded by the host, mirroring how the
  interface `Keymap` already takes a key *token*.
- **`IntentSnapshot`** (internal, behind `InputState`) — the resolved per-tick
  result: held-action bitset, down/up edge sets, last-press tick per action,
  pointer, and completed swipe. This snapshot — **not** the raw `DeviceFrame` — is
  the tick-indexed intent stream that is recorded, replayed (§17.4), and
  net-serialized per player (SPEC-13).
- **`SwipeDir`** — `{ Up, Down, Left, Right }`, the discriminated form the touch
  synthesizer's unit `Vec2` is quantized into at the boundary.

## 6. Determinism (sim; §17.3 is the spine of this spec)

- **Sampling boundary.** Raw events are impure and arrive at presentation rate;
  the **`sample` call at each tick boundary is where they become a per-tick
  intent snapshot.** Everything downstream (`is_down`/`pressed`/… and the whole
  sim) reads only that snapshot, indexed by `Tick` — never a live event, never
  wall-clock. This is exactly what makes input replayable and net-serializable:
  record the `IntentSnapshot` stream and the sim reproduces bit-for-bit.
- **Edge detection is pure tick arithmetic.** `pressed`/`released` are the
  set-difference of this tick's down-set against the previous tick's, computed
  branchlessly over bitsets; **auto-repeat is suppressed structurally** — a held
  key produces `pressed` only on the single transition tick, because the edge is a
  transition, not a level. No timer, no debounce clock.
- **`pressedAtTick` is the tick stamp of the most recent down-edge**, written
  during `sample`. Because it is a tick (not a timestamp), a rhythm/reaction game
  judges `tick - pressedAtTick(a)` against a fixed tick window deterministically;
  two replays of the same intent stream judge identically.
- **Single clock, single stream.** No wall-clock or frame-delta reaches any read;
  the only time is `Tick`. Ties to SPEC-13: `NetSim.inputOf(player)` is an
  `Input` over that player's snapshot stream; the per-player intent the wire
  carries (§16.2) is this `IntentSnapshot` shape, so the same determinism holds
  across authority and predicted instances.
- Branchless, 100% covered like all sim spine.

## 7. Acceptance / proof

- **Coverage & branchless.** `axiom-input` stays 100% covered and passes
  `engine_no_branching` after the extension — including the new bitset edge logic.
- **Edge semantics.** Tests: a key held across N ticks yields `pressed` on exactly
  one tick and `isDown` on all N; release yields `released` on exactly one tick;
  `axis(neg,pos)` returns `-1/0/1` for all four held combinations.
- **`pressedAtTick`.** A press at tick T reads `T` for the whole window after,
  re-stamps on a later press, and a `null` before any press.
- **Replay/golden (sim obligation §17.4).** Drive `sample` with a fixed
  `DeviceFrame` sequence twice; assert byte-identical `IntentSnapshot` streams and
  identical per-tick reads — and that the snapshot stream alone (no raw events)
  reproduces them. Cross-chunk invariance with the SPEC-00 accumulator: the same
  events grouped into ticks differently still produce the same per-tick snapshots.
- **Binding relocation.** The action table resolves the same first-match result
  the interface `Keymap` does for the sim-relevant cases, with no dependency on
  `axiom-interface`.
- **TS projection.** `@axiom/game`'s `Input`/`bindAction` pass tsgo + Oxlint
  (branch ban) + 100% TS coverage; a headless authored game asserts edge reads and
  `pressedAtTick` reproduce on a second run.

## 8. Dependencies & order

- **Depends on SPEC-00** for the `Sim.input` seam, the `Tick` type, and the wasm
  boundary that hands each tick's `DeviceFrame` to `sample`. The host/windowing
  edge that produces neutral `DeviceFrame`s is owned by SPEC-12.
- **Build order (contract §18 item 5):** lands after foundations (00–03), before
  or alongside grid/pathfinding (SPEC-06). **SPEC-13 depends on this** — per-player
  intents are this snapshot shape; do not finalize the wire intent before the
  `IntentSnapshot` contract here is fixed.

## 9. Open questions

- **Charge/hold-release and buffered direction — primitive vs derived?** Both are
  *temporal* patterns over the per-tick reads. Charge is `tick - pressedAtTick(a)`
  sampled at the release edge — derivable from the primitives above, so likely an
  author/feature-module helper, not module state. Buffered direction (hold the
  last directional intent for a few ticks so a slightly-early input still fires) is
  a small fixed-window ring of recent snapshots — it needs *dedicated state*. Open
  question: does the buffer live in `axiom-input` as a first-class
  `buffered(action, withinTicks)` read, or in a SPEC-07 (timers/state) feature on
  top? Lean: expose `pressedAtTick` + a thin buffer read here; keep charge derived.
- **Sharing the binding-match core with `axiom-interface`.** Both do guard-aware
  first-match key→action lookup. They differ only in guards (UI text-field/console
  routing vs none). If a third consumer appears, the match core is a candidate for
  a lower home — but it is **not** a kernel primitive (it is input-domain), so
  duplicating the tiny matcher is preferable to a manufactured shared edge today.
- **Touch analog intent's place in `Input`.** `ControlFrame`'s move/look is
  first-person *camera* intent with no slot in contract §8. Does it project as an
  app-level controller mapping (current behavior, kept behind the facade), or does
  §8 eventually grow a normalized 2-axis stick accessor? Default: keep it
  app-mapped until a game needs analog as a named action axis.
