# End Zone — controls

## Menus (all devices)

The frontend uses a device-independent action model — every device drives
the same navigate / confirm / cancel / pause actions, and the footer hints
track whichever device you used last.

| Action | Keyboard | Gamepad | Pointer / touch |
|---|---|---|---|
| Navigate | `W A S D` / arrows | D-pad / left stick | hover / tap |
| Confirm | `Enter` | `A` | click / tap |
| Cancel (back) | `Escape` | `B` | — (BACK buttons on-screen) |
| Pause | `P` | `Start` | on-screen `PAUSE` button |

Menu bindings are rebindable in SETTINGS → CONTROLS, but `Enter`, `Escape`,
and the arrows always work in menus (the emergency path), so no binding
state can lock you out. Cancel walks backward: match setup → team select
(opponent → player stage) → main menu → title. Idling ~30 s on the title or
main menu starts attract mode; any input returns.

## In-match — touch (mobile)

The virtual joystick (bottom-left) and buttons (bottom-right) appear only
while a match is live; they work with touch, pen, and mouse.

| Control | Action |
|---|---|
| Joystick | Steer YOUR player — the quarterback while he holds the snap, then the ball carrier after the catch. Stick up = downfield, stick right = the offense's right. Release it and the AI resumes. |
| `SNAP · THROW` | Contextual: snaps the ball pre-snap, throws while the quarterback holds it, restarts after the play ends. **The quarterback never throws on his own** — hold the ball too long and the rush will sack you. |
| `PAUSE` | Open the pause menu (RESUME / RESTART MATCH / SETTINGS / RETURN TO MAIN MENU). |

A connected gamepad's left stick also steers, and `A` is the contextual
snap/throw.

## In-match — keyboard (desktop)

| Key | Action |
|---|---|
| `W A S D` / arrows | The movement stick (same player-steering as the joystick) |
| `Enter` | The contextual snap / throw / restart action (rebindable: SNAP/THROW) |
| `P` / `Escape` | Pause |
| `Space` | Start the play now, or restart it after completion (diagnostic) |
| `R` | Reset the play to formation (diagnostic) |
| `1`–`4` | Force a camera: formation / quarterback / flight / carrier (diagnostic) |
| `5` | Return to automatic camera direction |
| `F1` | Toggle the diagnostic overlays (routes, steering targets, collision circles, catch volume, trajectory prediction, camera aim) |
| `` ` `` | Toggle the engine debug overlay panel (tick, phase, ball state, possession, camera mode, seed, impulses, QB role) |

The ball in flight, the defense, and downed players are never user-driven;
the same deterministic controller limits (acceleration, turn rate, boundary
clamp) apply to steered movement. The throw is always yours — a quarterback
left holding the ball gets sacked.
