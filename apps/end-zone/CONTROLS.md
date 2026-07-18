# End Zone — controls

The controls are fixed and read-only (there is no rebinding). The Controls
screen renders this same list from the fixed input map
(`src/frontend/bindings.rs`).

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
back through, and no attract mode.

## In-game — touch (mobile)

The virtual joystick (bottom-left) and buttons (bottom-right) appear only while
a run is live; they work with touch, pen, and mouse.

| Control | Action |
|---|---|
| Joystick | Steer YOUR player — the quarterback while he holds the snap, then the ball carrier after the catch. Stick up = downfield, stick right = the offense's right. Release it and the AI resumes. |
| `SNAP · THROW` | Contextual: snaps the ball pre-snap, throws while the quarterback holds it. **The quarterback never throws on his own** — hold the ball too long and the rush sacks you (the dead-ball clock blows the play dead). |
| `PAUSE` | Open the pause menu (RESUME / RESTART RUN / SETTINGS / CONTROLS / RETURN TO TITLE). |

A connected gamepad's left stick also steers, and `A` is the contextual
snap/throw.

## In-game — keyboard (desktop)

| Key | Action |
|---|---|
| `W A S D` / arrows | The movement stick (same player-steering as the joystick) |
| `Enter` | The contextual snap / throw action |
| `P` / `Escape` | Pause |

Diagnostic keys (not gameplay, never shown in a menu): `1`–`5` force / release a
camera mode, `F1` toggles the diagnostic overlays.

The ball in flight, the defense, and downed players are never user-driven; the
same deterministic controller limits (acceleration, turn rate, boundary clamp)
apply to steered movement.
