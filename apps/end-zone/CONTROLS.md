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
back through and no attract mode.

## In-game — calling the play (keyboard)

Every attempt starts at the line with the play card up, and **nothing happens
until you call one**. There is no clock on it: the game waits.

| Key | Action |
|---|---|
| `1` | Call **TRIPLE READ** — slant · dig · post |
| `2` | Call **FLOOD** — flat · corner · go |
| `3` | Call **MIRROR** — hitch · cross · wheel |

The moment you call, the offense sprints into that play's formation. The ball
snaps as soon as every man is on his spot, so **calling the play is the snap
count** — a bigger shift is a longer walk, and re-calling the formation already
on the field snaps at once. The number keys keep the same grammar all attempt:
they are the three plays here, and the three reads once the ball is live.

## In-game — the decision window (keyboard)

You do not steer the play. The offense snaps, drops back and runs its routes on
its own; you watch. At the moment the read is worth making, the game asks you
exactly one question.

| Key | Action |
|---|---|
| `1` | Throw to the **short read** (slant / flat / hitch, by play). Almost always there, worth almost nothing. |
| `2` | Throw to the **intermediate read** (dig / corner / cross). A chunk, through traffic. |
| `3` | Throw to the **deep read** (post / go / wheel). The big play, if you have the time. |
| `Space` | **Scramble** — abandon the pocket. You get the quarterback, and the defense knows it instantly. |
| *(nothing)* | Let the window close. The play runs on, the rush keeps coming, and you get one more (shorter) look. |

One press throws it. There is no meter to fill and no power to get wrong — the
pass is always on the money, so the only way to be wrong is to have read the
field wrong. Read `1` is always the earliest and safest and read `3` always the
latest and largest, in every play, so the mapping survives changing your call.

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

**There is no virtual joystick.** The game does not ask you to steer, so a
thumbstick would advertise a verb it does not have; a touch carrier simply runs
on his own AI intent.

The prompts *are* the buttons. A delegated pointer listener reads the tapped
element, so every on-screen chip is a real control on touch, pen, and mouse —
one piece of UI in the place you are already looking, which is why the touch and
keyboard controls can never disagree.

| Control | Action |
|---|---|
| A play row on the card | Calls that play — the twin of `1`/`2`/`3` at the line. |
| A read chip | Throws to that read — the twin of `1`/`2`/`3` once the ball is live. |
| `SCRAMBLE` | Abandons the pocket (the twin of `Space`). |
| `PAUSE` | Open the pause menu (RESUME / RESTART RUN / END SESSION / SETTINGS / CONTROLS / RETURN TO TITLE). |

## Diagnostics

Never gameplay, never shown in a menu: `F1` toggles the diagnostic overlays,
`F2`–`F5` force a camera mode and `F6` returns to automatic direction. Camera
forcing used to live on `1`–`5`; it moved when the number row became the reads.

The ball in flight, the defense, and downed players are never user-driven; the
same deterministic controller limits (acceleration, turn rate, boundary clamp)
apply to steered movement.
