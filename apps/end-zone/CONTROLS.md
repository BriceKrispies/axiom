# End Zone — controls

## Touch (mobile)

The page mounts a virtual joystick (bottom-left) and two buttons
(bottom-right); they work with touch, pen, and mouse.

| Control | Action |
|---|---|
| Joystick | Steer YOUR player — the quarterback while he holds the snap, then the ball carrier after the catch. Stick up = downfield, stick right = the offense's right. Release it and the AI resumes. |
| `SNAP · THROW` | Contextual: snaps the ball pre-snap, orders the throw while the quarterback holds it, restarts after the play ends |
| `RESET` | Reset all showcase state to formation |

The ball in flight, the defense, and downed players are never user-driven;
the same deterministic controller limits (acceleration, turn rate, boundary
clamp) apply to steered movement.

## Keyboard (desktop)

| Key | Action |
|---|---|
| `W A S D` / arrows | The movement stick (same player-steering as the joystick) |
| `Enter` | The contextual snap / throw / restart action |
| `Space` | Start the showcase play now, or restart it after completion |
| `R` | Reset all showcase state to formation (idle until started) |
| `1` | Force the formation-wide camera |
| `2` | Force the quarterback-follow camera |
| `3` | Force the football-flight camera (only while the ball is airborne) |
| `4` | Force the ball-carrier-follow camera (only while possession exists) |
| `5` | Return to automatic camera direction |
| `F1` | Toggle the diagnostic overlays (routes, steering targets, collision circles, catch volume, trajectory prediction, camera aim) |
| `` ` `` | Toggle the engine debug overlay panel (tick, phase, ball state, possession, camera mode, seed, impulses, QB role) |

On load the showcase arms itself: the play starts automatically after a short
deterministic delay and runs by itself if you never touch anything. Steering
or pressing `SNAP · THROW` takes over at any point; on touch devices the
debug overlay panel stays closed by default (Backquote opens it).
