# End Zone — diagnostic controls

These are DIAGNOSTIC controls for the systems showcase. There are no gameplay
controls yet.

| Key | Action |
|---|---|
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
deterministic delay, snaps, throws on schedule, and resets to `Done` after the
tackle — press `Space` to run it again.
