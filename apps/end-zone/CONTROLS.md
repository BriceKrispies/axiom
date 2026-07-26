# End Zone — controls

The controls are fixed and read-only (there is no rebinding). The Controls
screen renders this same list from the fixed input map
(`src/frontend/bindings.rs`); the in-game half lives in `src/controls.rs`.

## Menus (all devices)

The frontend uses a device-independent action model — every device drives the
same navigate / confirm / cancel / pause actions, and the footer hints track
whichever device you used last.

| Action | Keyboard | Gamepad | Pointer / touch |
|---|---|---|---|
| Navigate | `W A S D` / arrows | D-pad / left stick | hover / tap |
| Confirm | `Enter` | `A` | click / tap |
| Cancel (back) | `Escape` | `B` | on-screen `BACK` |
| Pause | `P` | `Start` | on-screen `PAUSE` button |

`Enter`, `Escape`, and the arrows always work in menus (the emergency path).
Cancel is consistent: `RESUME` from the pause menu, `BACK` from settings and
controls. The title leads straight into gameplay — there is no menu tree to walk
back through, no attract mode, and **no play-call screen**.

## In-game — the decision window (keyboard)

You do not steer the play. The offense snaps, drops back and runs its routes on
its own; you watch. At the moment the read is worth making, the game drops into
slow motion and asks you exactly one question. You have a couple of real seconds.

| Key | Action |
|---|---|
| `1` | Throw the **SLANT** — the short read. Almost always there, worth almost nothing. |
| `2` | Throw the **DIG** — the intermediate crosser. A chunk, through traffic. |
| `3` | Throw the **POST** — the deep read. The big play, if you have the time. |
| `Space` | **Scramble** — abandon the pocket. You get the quarterback, and the defense knows it instantly. |
| *(nothing)* | Let the window close. The play runs on, the rush keeps coming, and you get one more (shorter) look. |

Each receiver wears a coloured ring and a stack of floating cubes: one cube for
read `1`, two for `2`, three for `3`. Colours identify **who**, never whether the
throw is a good idea — reading the coverage is the game.

Waiting is the whole mechanic. Later routes come open and the deep shot is worth
more, but every window you decline is shorter than the last, and after the third
one nobody asks again — the rush simply gets home and you are sacked.

## In-game — after the decision

| Key | Action |
|---|---|
| `W A S D` / arrows | Steer the ball carrier — the scrambling quarterback, or the receiver after the catch |
| `P` / `Escape` | Pause |

The stick does nothing before you have decided: while the play develops the
simulation owns every player, which is the premise the prototype exists to test.

## In-game — touch (mobile)

The virtual joystick (bottom-left) and buttons (bottom-right) appear only while
a run is live; they work with touch, pen, and mouse.

| Control | Action |
|---|---|
| Joystick | Steer the ball carrier once you have committed (scramble or after the catch). |
| `SNAP · THROW` | During a decision window, commits the **highlighted** read — the one-button twin of the numbered keys. |
| `PAUSE` | Open the pause menu (RESUME / RESTART RUN / END SESSION / SETTINGS / CONTROLS / RETURN TO TITLE). |

Touch has no per-read buttons yet, so mobile can only take the highlighted read
or ride the window out. That is a known gap, not a design choice.

## Diagnostics

Never gameplay, never shown in a menu: `F1` toggles the diagnostic overlays,
`F2`–`F5` force a camera mode and `F6` returns to automatic direction. Camera
forcing used to live on `1`–`5`; it moved when the number row became the reads.

The ball in flight, the defense, and downed players are never user-driven; the
same deterministic controller limits (acceleration, turn rate, boundary clamp)
apply to steered movement.
