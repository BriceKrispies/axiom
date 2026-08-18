# `input` — `src/core/input.js` → `apps/shmup/src/input.rs`

The last unported file of `src/core/`. 253 lines of source, ported whole.

## What is here

`ACTIONS` (all 17 entries, in source order) as an `Action` enum plus a
`[(Action, &[&str]); 17]` table; `Input`, the per-frame snapshot model —
`down`/`_pressed`/`_released`/`_pendingDown`/`_pendingUp` as `BTreeSet<String>`,
`look`/`_rawLook`, `wheel`/`_pendingWheel`, `pointerLocked`/`enabled`/`frozen`,
and `stick`; `beginFrame` (edge resolution, sensitivity + `invertY`, wheel latch,
gamepad poll), `endFrame` (the source's no-op, kept so the call site reads the
same), `_pollGamepad`, `action`/`actionPressed`/`held`/`pressed`/`released`, the
`fire`/`firePressed`/`ads` getters, and `moveVector`.

## Divergences, and why

1. **`config` is a per-frame argument, not a field.** The source stores the
   config object it was constructed with. The port passes `&Config` into
   `begin_frame`, so the live config the settings menu edits is always the one
   applied; a stored copy would have to be invalidated by hand.
2. **`_pollGamepad`'s `navigator` read is lifted to an argument.** `begin_frame`
   and `poll_gamepad` take `Option<[f64; 4]>` — the four axes the source
   actually reads. `dom::poll_pad()` (wasm32) is what calls
   `navigator.getGamepads()`. Everything the source *decides* with those axes —
   the 0.16 dead zone and its rescale, the `^2.4` look curve — stays in the
   natively-tested half.
3. **`requestPointerLock` is asked for, not made.** `_onMouseDown` requests the
   lock; that is a browser call. `Input::wants_pointer_lock(button)` returns the
   source's condition (`!pointerLocked && button === 0`) and `dom` makes the
   call. The source's try/catch around the rejected promise has no analogue —
   `web_sys`'s `request_pointer_lock` returns `()`.
4. **`e.repeat` is filtered by the caller.** It is a property of the DOM event,
   not of the snapshot model, so `dom`'s keydown listener drops repeats before
   calling `key_down`.
5. **`detach()` is not ported.** The listeners are `Closure::forget`ten: they
   live exactly as long as the page, and this app never tears its input down.
   Porting `detach` would mean retaining nine closures nothing would ever drop.

## The `Math.sign` trap

`js_sign` is hand-rolled. `f64::signum(0.0)` is `1.0` (and `-1.0` for `-0.0`)
where JS `Math.sign(0)` is `0`. It matters in two places here: the wheel
accumulator (`_pendingWheel += Math.sign(e.deltaY)`, where a `deltaY` of exactly
0 must contribute nothing) and the dead-zone rescale. Pinned by
`js_sign_returns_zero_for_zero_unlike_signum`.

## What it is verified against

No golden capture: there is no pure function here whose output a Node script
could dump — every routine is a transition on mutable snapshot state driven by
DOM events. Instead the port is pinned by 19 unit tests that assert the *model*,
each named for the source behaviour it holds: the press/hold/release edge
lifetime, a down-and-up inside one frame producing both edges, blur releasing
every held key, pointer-lock loss blurring, the sensitivity and `invertY`
scaling, `frozen` zeroing the look without disturbing keys, the wheel latch, the
dead zone being *rescaled* rather than clipped, the cubic look curve, and
`moveVector` clamping a diagonal to the unit disc (`hypot == 1`, not `sqrt(2)`).

## The seam it closes

`impl crate::player::movement::PlayerInput for Input` — the four methods
`movement.js`'s `latchInput` calls. That is the whole `ctx.input` seam
`crate::player`'s module doc named as unported; it is now bound.
