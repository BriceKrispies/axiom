# End Zone — frontend

The End Zone frontend (`src/frontend/*`) is a small, pure, browser-free shell
over the deterministic run. The platform edge feeds one neutral input frame per
tick and renders the returned `SceneView` (plus the gameplay HUD, which the edge
builds from authoritative run state). Everything else — the screen state
machine, focus, settings, persistence, theme — lives here with zero browser
types, so the whole frontend is native-testable.

There is no attract mode, team selection, match setup, or credits. The title is
a start plate; a first press opens a small **Menu** (`PLAY` / `SETTINGS`). That
first press doubles as the browser gesture that starts the menu music (the music
plays on the `Menu`, not the bare title — see `web/music.rs`).

## The seven-state machine

`src/frontend/screen.rs` — exactly seven states, never booleans:

```
Title  →(confirm)→  Menu  →(play)→      InGame  →(pause)→  Paused
                     ├─(settings)→ Settings →(back)→ Menu    ├─(resume)→       InGame
                     └─(back)→     Title                     ├─(settings)→     Settings →(back)→ Paused
                                                             ├─(controls)→     Controls →(back)→ Paused
                                                             ├─(restart run)→  InGame (fresh)
                                                             └─(return)→       Title
InGame →(failed 4th-down conversion)→ GameOver
                                        ├─(play again)→   InGame (fresh, new seed)
                                        └─(return)→       Title
```

Settings/Controls are shared between the pre-game `Menu` and the in-game
`Paused` menu; `FrontendState::sub_return` records which opened them so `back`
returns to the right one (`screens::back_from_sub`).

Transitions are explicit, recorded methods on `FrontendState`; the frontend
answers the composition layer only through drained `FrontendCommand`s
(`LaunchRun{seed}` / `RestartRun` / `ReturnToTitle` / `SetPaused` /
`EndSession`). The attempt loop is endless, so there is no automatic game over:
the pause menu's END SESSION emits `EndSession`, and the shell answers by
pushing the session summary **in** (`FrontendApp::enter_game_over`). The
frontend never queries the simulation.

There is no play-call screen. The old pre-snap `Huddle` went with the drive
loop — the prototype runs one concept and asks its only question on the field,
inside the decision window (`tests/frontend_reduction.rs` keeps it gone).

## Title

Only the procedural `END ZONE` mark and a blinking `PRESS START` prompt, over
the live ambient field showcase. Any confirm opens the `Menu` (a fade). No team
names, cards, ratings, difficulty, credits, or match setup.

## Menu

The small pre-game menu behind the title: exactly `PLAY` and `SETTINGS` over the
attract field. `PLAY` rolls a fresh explicit run seed and starts the run
(`LaunchRun{seed}` + a wipe); `SETTINGS` opens the shared settings screen
(returning here on `back`); `back` returns to the title. This is the screen the
menu music plays on.

## Pause

Exactly five actions over the frozen run: `RESUME`, `RESTART RUN`, `SETTINGS`,
`CONTROLS`, `RETURN TO TITLE`. No confirmation dialogs — restart and return act
immediately. Pausing emits `SetPaused(true)`; the shell stops advancing the
simulation, so no delta accumulates and menu animation stays responsive.

## Game over

`RUN OVER`, the run summary (final score, touchdowns, first downs, longest
play), and exactly `PLAY AGAIN` / `RETURN TO TITLE`. Play again rolls a fresh
explicit seed (tests can pin the base seed); return disposes of the run.

## Minimal HUD

The in-game HUD (`src/presentation/hud.rs`, rendered by the edge in a separate
DOM layer) shows only: score (`SCORE 012500`), down + distance (`2ND & 6`,
`1ST & GOAL`), the line-to-gain indicator (`TO GAIN 6` / `GOAL LINE`), and heat
(`HEAT 3`). Every value is derived from authoritative `DriveState` — the HUD
keeps no counters of its own. There are no team ratings, player stats,
possession, clock, quarter, opponent score, minimap, or dashboards. The
line-to-gain is also drawn on the field as a bright marker (`src/scene.rs`).

## Fixed teams

Two fixed fictional teams (`src/data/team.rs`): CRATER CITY **MAGMA** on offense
(the player), GLACIER FALLS **FROSTBITE** on defense. They are pure data
(ratings + palette) the generic systems scale — there are zero team branches in
code, and no user-facing team selection. `RunConfig` always carries these two
ids; there is no way to pick, lock, or swap teams.

## Compact persistence

`src/frontend/persistence.rs` persists only the three settings (see
`SETTINGS.md`) as a small versioned `key=value` text behind the app-local
`ProfileStore` trait. Loaded values are validated and fall back per field to
defaults; a persistence failure logs through the kernel logger and never blocks
the title or gameplay. No team selections, difficulty, camera, focus, or run
state are persisted.

## Input

`src/frontend/input.rs` translates each neutral device frame into
device-independent actions (navigate, confirm, cancel, pause, pointer
move/activate). Every screen has deterministic initial focus, visible focus, and
consistent confirm / cancel across keyboard, gamepad, pointer, and touch. The
control map (`src/frontend/bindings.rs`) is fixed — the Controls screen renders
it read-only; there is no rebinding.
