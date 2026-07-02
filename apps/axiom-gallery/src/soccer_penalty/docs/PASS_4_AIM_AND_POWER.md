# Pass 4 — Deterministic Aim Reticle & Shot Power Interaction

Passes 1–3 built a static, deterministically-ordered, flat-shaded diorama. Pass 4
makes it *interactive*: the player can aim a reticle across the goal mouth and
charge a shot-power meter, and the HUD reflects both. **The ball does not move,
there is no goalie reaction, and there is no scoring** — this is aim/power intent
only.

Still fixed-camera. Still fully deterministic: fixed-tick integer arithmetic, no
wall-clock time, no randomness, no unordered maps.

## What Pass 4 adds

- **`PenaltyInputIntent`** (`penalty_input.rs`) — the app-local deterministic
  input contract.
- **`PenaltyInteractionState`** (`penalty_interaction.rs`) — a fixed-tick state
  machine over `Aiming` / `Charging` / `LockedPreview`, holding
  `PenaltyAimState`, `PenaltyPowerState`, a local tick, and an optional
  `PenaltyShotPreview`.
- **`PenaltyShotPreview`** — the frozen aim + power captured on release (a
  *preview / locked intent*, not a shot result).
- **`PenaltyHudModel`** (`penalty_hud.rs`) — now derived from the interaction
  state: the power meter and aim reticle reflect live values, and an instruction
  label reflects the phase. Still unlit, still rendered last.
- **`SoccerPenaltyApp::build_frame(&state)`** — builds the diorama + render plan
  + HUD for a given interaction state (`build_stage1()` is the start state).

## The deterministic input intent model

`PenaltyInputIntent` is one fixed struct — the abstract "what the player asked
for this tick", decoupled from any device:

| Field            | Type   | Range / meaning                          |
|------------------|--------|------------------------------------------|
| `aim_x_axis`     | `i32`  | `-100..=100` (negative left, positive right) |
| `aim_y_axis`     | `i32`  | `-100..=100` (negative down, positive up)    |
| `charge_pressed` | `bool` | holding the shot button (charge power)   |
| `release_pressed`| `bool` | released the shot this tick (lock)       |
| `reset_pressed`  | `bool` | reset aim + power to the start           |

It reads **no** browser/host APIs. It is the deterministic contract tests drive
and future host wiring targets.

## The aim target coordinate system

The reticle lives in normalized **target space**, clamped to the goal-mouth
rectangle:

- `x ∈ [-100, 100]` (0 = goal center, ±100 = posts),
- `y ∈ [0, 100]` (50 = center height, 100 = crossbar, 0 = ground).

It starts centered at `(0, 50)`. Each tick it moves by
`axis * AIM_RATE / 100` (with `AIM_RATE = 8`, i.e. 8 units/tick at full
deflection), then clamps back into the rectangle — so it can never leave the
goal. `PenaltyHudModel` maps this target to a normalized on-screen position over
the goal so the existing HUD reticle can display it; the reticle render item
stays in the `Hud` layer with a stable ordinal.

## The power meter state model

`PenaltyPowerState.power ∈ [0, 100]`, starting at `0`. While `charge_pressed`,
power rises by `CHARGE_PER_TICK = 8` per tick and clamps at `100`. On
`release_pressed` the current power (and aim) freeze into a `PenaltyShotPreview`.
Power uses **fixed ticks only** — never wall-clock duration.

## The interaction states

```
          charge_pressed                 release_pressed
 Aiming ──────────────────► Charging ────────────────────► LockedPreview
   ▲          (power += 8/tick, aim still moves)                 │
   │                                                             │
   └──────────────────── reset_pressed ◄────────────────────────┘
                    (aim → center, power → 0)
```

- **Aiming** — reticle can move; power is `0`.
- **Charging** — reticle can still move; power increases while charge is held.
- **LockedPreview** — reticle and power are frozen after release; the ball still
  does not move. Only `reset_pressed` leaves this state.
- `reset_pressed` always returns to a fresh `Aiming` (centered aim, zero power,
  cleared preview), from any state.

`release` wins over `charge`; `charge` wins over "hold". The whole rule engine is
the pure function `PenaltyInteractionState::advance(intent) -> Self`, and
`run(&[intent])` folds a whole sequence from the start — identical sequences
yield byte-identical states.

### Worked example (the scripted test)

Aim right 5 ticks → `x = 40`. Aim up 3 ticks → `y = 74`. Hold charge 12 ticks →
`power = 96`. Release on tick 21 → `PenaltyShotPreview { target_x: 40,
target_y: 74, power: 96, release_tick: 21 }`.

## Why ball flight is deliberately not implemented yet

Pass 4's job is to lock a deterministic *intent* — where the player aimed and how
hard they hit it — nothing more. Ball flight needs a trajectory model,
collision/goal geometry, and a goalie reaction, all of which have their own
determinism and test surface. Committing to a trajectory now would entangle those
concerns before the intent contract is proven. The `PenaltyShotPreview` is
exactly the stable, frozen descriptor Pass 5 will consume to launch the ball —
built and tested here first, in isolation.

## Why browser input is not read directly in the app core

Reading `window`/`document`/gamepad APIs inside the app core would (a) break
determinism (device timing, event coalescing) and (b) violate the engine's
platform-edge rule. So the core consumes only `PenaltyInputIntent`, a pure data
struct, which keeps the whole interaction model replayable and unit-testable with
no browser present.

## How future host/browser wiring should translate real input

A future host/browser adapter (the platform edge, e.g. `axiom-host` /
`axiom-windowing`, or a thin app-side input reader) should, per frame:

1. sample the real device (keys / gamepad stick / touch drag),
2. quantize the aim into `aim_x_axis` / `aim_y_axis` in `-100..=100`,
3. set `charge_pressed` while the shot control is held, `release_pressed` on the
   release edge, and `reset_pressed` on the reset control,
4. call `PenaltyInteractionState::advance(intent)` once per fixed tick.

The app core never sees the device — only the intent — so the same logic runs
identically in a browser, a native harness, or a headless test.

## Still not implemented (later stages)

Pass 4 is aim + power intent only. It deliberately does **not** add:

- a ball trajectory / ball flight;
- goalie animation or dive poses;
- collision volumes of any kind;
- save / goal / miss / post resolution;
- net wobble or impact effects;
- scoring, round, or best-score changes (the scoreboard stays static).

The ball is still on the spot, the camera is still fixed, and no real physics or
dynamic shadows exist. See `STAGE_1.md` for the full roadmap.
