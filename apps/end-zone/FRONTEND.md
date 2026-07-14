# End Zone — the front-end shell

The production menu layer over the deterministic showcase: an original
early-2000s arcade-sports interface, built app-locally on the engine's
`interface` + `layout` layers. The gameplay systems (field, players,
football, AI, camera, contact, juice) are untouched — the frontend is a
separate, pure, native-testable machine that talks to them across one typed
boundary.

## Architecture: pure core, dumb edge

```text
DOM listeners / gamepad poll / touch      (src/web/ — wasm32 only)
  → FrontendInputFrame                    neutral tokens + pointer
    → InputTranslator                     device-independent actions
      → FrontendState + screens           the explicit state machine
        → SceneView                       typed, positioned view model
  ← MenuPresenter                         renders SceneView as DOM
  ← FrontendCommand                       launch / restart / return / pause
      → EndZoneShell                      applies commands to the game
```

* **`src/frontend/`** is browser-free and fully native-testable. It owns the
  screen state machine, focus, settings, persistence encoding, theme,
  transitions, widgets, and audio recipes. It never touches the simulation.
* **`src/shell.rs`** composes `FrontendApp` over `EndZoneApp`. It drains the
  frontend's typed `FrontendCommand`s and drives the sim per the frontend's
  `SimDirective` (`Menu` = ambient showcase, `Live` = the match, `Frozen` =
  paused). The sim never queries the frontend.
* **`src/web/`** is the sanctioned nondeterministic edge: the DOM presenter
  (`presenter.rs`/`markup.rs`/`style.rs`/`emblem.rs`), the storage adapter
  (`storage.rs`, the ONLY place browser storage is touched), gamepad polling
  (`gamepad.rs`), menu tones (`tones.rs`), and the in-match touch controls
  (`touch.rs`).

## The screen state machine

Eleven explicit states (`frontend/screen.rs`) — never booleans:

`Attract, Title, MainMenu, TeamSelect, MatchSetup, Settings, Credits,
TransitionToGame, InGame, Paused, TransitionToMenu`

Every transition is a recorded `FrontendState::go` (bounded history, replay
compared in tests). Cancel walks backward consistently: MatchSetup →
TeamSelect (stage 2 → stage 1 → MainMenu) → Title. Settings is reachable
from MainMenu AND Paused; it records its origin and returns to it with the
originating menu's focused item restored (per-screen focus memory).

Attract mode enters after ~30 s of inactivity on Title/MainMenu only, runs
the REAL deterministic showcase behind the mark (no video, no recording),
and exits to Title on any input. The inactivity clock lives entirely in the
frontend.

## The action model

Screens never see key codes. All devices translate into
`FrontendAction::{Navigate, Confirm, Cancel, Pause, PointerMove,
PointerActivate}` stamped with an `InputDevice` (keyboard / gamepad /
pointer / touch):

* keyboard codes and `Pad*` gamepad tokens flow through the rebindable
  `ControlBindings` (with a permanent emergency path: Enter / Escape /
  arrows always work in menus);
* navigation repeats with an explicit delay (18 ticks) and cadence (7);
* pointer hover focuses, pointer press activates; a touch pointer flips the
  hints to touch labels;
* the navigation-hint device is a stable last-active-device policy — one
  stray pointer event never flickers the hints.

Focus is a deterministic per-screen grid (`FocusList`): nearest enabled
entry strictly in the pressed direction, primary-axis first, declaration
order as the final tie-break. Modals confine focus to their options.

## The launch boundary

`MatchLaunchConfig` (`src/launch.rs`) is frozen at START MATCH: both teams,
home/away, field id, difficulty, game speed, camera style, deterministic
seed, presentation profile (effects / shake / flash), control profile. It is
validated (`SameTeams`, unknown ids, unknown profile), then resolved ONCE by
`resolve_launch` into the sim-facing `MatchSetup` (rosters scaled by team
ratings, difficulty applied to the opponent's defensive DATA — zero team or
difficulty branches in gameplay code). Restarting re-resolves the same
config: byte-identical initial state, proven by test.

Game speed never changes the fixed step: it is a pure per-frame step count
(`Normal` 1, `Fast` 2/1 alternating, `Turbo` 2).

## Determinism

The frontend is synchronous and seeded: per-match seeds derive from a fixed
base seed + a match counter through a splitmix64 finalizer (shown on the
match-setup screen). Identical input scripts replay to identical screen
histories and scenes; menu input never reaches the ambient showcase (both
proven by test).

## Visual identity

All procedural: beveled steel plates, chrome-gradient END ZONE mark, angled
clip-path silhouettes, glows, a light sweep, scanline + vignette overlays,
squash-and-snap press animation, team-color card tints, and procedural SVG
emblems built from the typed `EmblemDefinition` vocabulary. No image files
anywhere. Reduced motion swaps sweeps/zooms for short fades and stills the
decorative motion; high contrast switches the computed palette; enhanced
color distinction adds non-color team cues.

## Tests

| File | Proves |
|---|---|
| `tests/frontend_flow.rs` | screen flow, cancel path, pause, settings origin, attract, replay determinism |
| `tests/frontend_focus.rs` | focus grid, memory, hover, repeat delay/cadence, modal confinement, device hints |
| `tests/frontend_teams.rs` | six valid unique teams, data-driven strengths, no duplicate selection |
| `tests/frontend_settings.rs` | working-vs-committed, apply/reset/discard, live preview, rebind capture |
| `tests/frontend_persistence.rs` | versioned round-trip, per-field fallback, store abstraction |
| `tests/frontend_launch.rs` | validation, profiles-as-data, pacing, byte-identical reproduction |
| `tests/frontend_shell.rs` | pause freeze, restart, return-to-menu, ambient-sim input isolation |

## Known limitations

* Music / match-effect / crowd volumes are typed and persisted but have no
  audible path yet — the engine's sample/music playback arm is a stub (see
  `SETTINGS.md`).
* Haptic intents are typed and recorded at the boundary but unsupported: no
  Axiom host abstraction for vibration exists, and the app does not call
  browser vibration APIs directly.
* One control profile exists (`ControlProfileId(0)`); the launch config and
  persistence already carry the identity for future profiles.
