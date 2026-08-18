# `weapons/clips.rs` — port notes

Source: `C:\dev\Claude-of-Duty\src\weapons\clips.js:1-318` (whole file).
Target: `apps/claude-of-duty/src/weapons/clips.rs` +
`apps/claude-of-duty/tests/weapons_clips_port.rs`.

## What was ported

- `EASE` → `Ease` enum (`Linear`/`Smooth`/`Out`/`Back`) with `Ease::apply`,
  reusing `mathx::{smootherstep, ease_out_cubic, ease_out_back}` (already
  landed in `mathx.rs`, commit `78f80aa8`) rather than redefining them.
- `sampleTrack` → `locate<K: Keyframe>`, returning `(index_a, index_b, weight)`
  instead of taking a blend closure — each of the three channels in
  `Clip::sample` does its own field-by-field `lerp` with those indices, the
  direct Rust analogue of the source's per-channel callback body.
- `class Clip` → `Clip` struct + `Clip::sample(&self, t, &mut SampleResult)`.
- `makeSampleResult()` → `SampleResult::default()` / `make_sample_result()`.
- `buildClips(nodes, def)` → `build_clips(&AttachNodes, &WeaponDef) -> Clips`,
  producing all five clips (`reload_tac`, `reload_empty`, `inspect`, `draw`,
  `holster`) with every keyframe's numeric literal transcribed unchanged.

### `AttachNodes` — a new, minimal rig contract

`clips.js` takes the *whole* `model.nodes` object from the not-yet-ported rig
(`viewmodel.js` / `models/*.js`), but only reads three things from it:
`nodes.gripL` (`pos`, optional `finger`/`back`), `nodes.magSeat.pos`, and the
optional `nodes.chargeRest.pos`. Rather than stub out the whole rig, this port
defines `AttachNodes { grip_l: GripNode, mag_seat: PosNode, charge_rest:
Option<PosNode> }` — exactly the surface `build_clips` touches. When
`viewmodel.js` is ported, its rig type should either become this shape or
translate into it at the call site.

`def.magLen ?? 0.2`, `def.reloadTac ?? 2.15`, etc. all default a *possibly
undefined* JS field; `WeaponDef`'s corresponding fields (`mag_len`,
`reload_tac`, `reload_empty`, `inspect_time`, `draw_time`, `holster_time`) are
non-optional and always populated for every real weapon, so those defaults are
unreachable in this port and are only documented as comments (same pattern
`defs.rs` already uses for its own dead JS defaults).

## Dead vocabulary in the source (not a bug, nothing to port)

The channel doc comment (`clips.js:15`) advertises a `parts` channel driving
`mag / magHand / charge / bolt / slide / trigger`, but the actual parts blend
in `Clip.sample` (`clips.js:87-95`) only reads/writes `mag`, `magVisible`,
`charge`, `bolt`, `slide`. `magHand` and `trigger` are never wired to any
track or blend, and no authored clip data sets them. Documented in the module
doc comment; nothing was ported for those two names because there is no
behavior behind them.

## Source defect ported and pinned (not fixed)

**The interesting one.** In `reloadTac`, `reloadEmpty` and `inspect`, the
*final* keyframe of the `weapon`, `lhand` and `parts` tracks is authored at
the literal `t: 1` instead of `t: 1 * scale` (`tac`/`emp`/`insp`) — every
other keyframe in the same track is properly scaled. For every weapon in
`defs.rs`, the relevant scale is always > 1 second (reload/inspect times run
1.6s–3.2s), so that final key's `t` (`1`) ends up *smaller* than the
second-to-last key's scaled `t` — the track's own keyframes are out of
chronological order.

`sampleTrack`/`locate` never re-sorts; it does a single forward scan
(`while keys[i+1].t <= t`). Once elapsed time first reaches the second-to-last
key's scaled time, that scan also satisfies `keys[last].t (== 1) <= t` (since
`t` is now past `1` too) and jumps straight to the final key. Because that
final key is both `a` and `b` in that case, the weight is 0 and the value is
exactly the final key's own fields — the channel **snaps instantly to its
rest pose**, skipping the tail of the authored animation (e.g. the
`back`-eased overshoot key at `0.78 * tac`) entirely. It never eases through
the last leg; it pops.

`draw` and `holster` are unaffected: `draw_time`/`holster_time` are always
< 1 second for every weapon, so the literal `1` legitimately is the largest
keyframe time in those tracks.

Pinned in `tests/weapons_clips_port.rs`:
- `reload_tac_weapon_channel_snaps_early_because_the_final_key_ignores_the_time_scale`
  samples just before, exactly at, and just after the boundary
  (`0.78 * tac`), showing the value still easing toward overshoot on one side
  and already exactly `[0, 0, 0]` on the other — plus well-past-boundary and
  past-duration samples staying frozen at neutral.
- `reload_tac_at_the_nominal_t_equals_one_is_not_yet_the_quirk_boundary` shows
  `t = 1.0` (the *literal* value, but nowhere near the *scaled* boundary for
  tac = 2.1) is still ordinary mid-segment interpolation, to make clear the
  bug is about the scaled second-to-last key's time, not about `t = 1` per se.

Per the port recipe (rule 7): behavior ported and pinned, not silently fixed.
A fix (scaling every final key by its clip's `tac`/`emp`/`insp`, or anchoring
it at `duration`) is a one-line change per track if a future task wants to
propose it — but that is a behavior change to the game, not a porting
decision, so it was left alone here.

## Golden capture

Node (v24) script run from within `C:\dev\Claude-of-Duty` (relative import;
an absolute `C:/...` import URL fails under Node's ESM loader on Windows —
`ERR_UNSUPPORTED_ESM_URL_SCHEME`) against `buildClips`/`Clip.sample` for:

- a synthetic rifle-shaped rig (`gripL`/`magSeat`/`chargeRest` all present)
  paired with `defs.rs::RIFLE`'s real handling numbers (`mag_len = 0.212`,
  `reload_tac = 2.1`, `reload_empty = 2.9`, `inspect_time = 3.2`,
  `draw_time = 0.62`, `holster_time = 0.4` — copied into the JS capture
  script's `rifleDef` literal to match `RIFLE` field-for-field), and
- a synthetic pistol-shaped rig with **no** `chargeRest`, to exercise the
  slide-rack `lhand` branch, paired with `defs.rs::PISTOL`'s real numbers.

Sampled at: before the track starts (negative `t`), exactly on keyframes,
between keyframes, inside a `back`-eased segment, at the quirk boundary
(before/at/after), at the nominal `t = 1` (not the same thing — see above),
and past the clip's duration. Event arrays were captured whole per clip
(pure `t * scale` arithmetic).

**Every assertion is exact `f64` equality**, not a tolerance: the whole
sampler (`lerp`, `clamp01`, `smootherstep`, `ease_out_cubic`, `ease_out_back`)
is built only from `+ - * /` and comparisons — no `sin`/`cos`/`ln`/`sqrt`/`exp`
anywhere on this path — so there is no libm cross-implementation risk to
tolerate, per the port recipe's rule 3.

The capture script (`capture_clips.mjs`) was deleted after capture, per the
recipe; the printed JSON was copied into the Rust test file's literal
constants and is not retained anywhere else.

## What was not ported

- The rig itself (`viewmodel.js`, `models/rifle.js`, `models/smg.js`,
  `models/pistol.js`) — out of scope for this task; `AttachNodes` above is
  the minimal seam clips.rs needed and is written to be easy to construct
  from that rig once it lands.
- Any event-firing/dispatch logic — `clips.js` itself has none;
  `this.events` is just stored data. Whatever consumes it
  (`viewmodel.js`/`ai/animator.js`) is not ported yet either.
