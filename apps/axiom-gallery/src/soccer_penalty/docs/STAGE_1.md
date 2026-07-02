# Stage 1 — Static Diorama

## Goal

Establish the penalty-kick game **visually** before any gameplay exists. Stage 1
produces a deterministic, fixed-camera diorama that reads at a glance as a
retro 32-bit-ish low-poly soccer penalty kick: kicker behind the ball, ball on the spot,
goalie in the goal, posts + net, a lined field, a stadium wall with a fake
crowd, and a chunky arcade HUD. Nothing animates.

Everything is a pure function of the constants in `penalty_scene.rs`,
`static_diorama.rs`, and `penalty_hud.rs`. There is no wall-clock time, no
randomness, and no unordered iteration, so the diorama rebuilds byte-for-byte
identically every time.

## Scene composition

Coordinate convention (app-local): `+X` right, `+Y` up, `+Z` from the goal
toward the kicker/camera. The goal line is at `z = 0`; the ball/penalty spot at
`z = 11`.

| Group        | Contents                                                                 |
|--------------|--------------------------------------------------------------------------|
| Field        | Large green plane + alternating light/dark grass bands                   |
| Markings     | Goal line, penalty box, goal area, penalty spot (thin white quads)       |
| Goal frame   | Two posts + crossbar (white boxes)                                       |
| Net          | Rear + front grids of thin line segments, on separate draw layers        |
| Kicker       | Low-poly primitive puppet (legs, shorts, torso, head, arms, hands)       |
| Ball         | Faceted sphere on the penalty spot (radius exaggerated for readability)  |
| Goalie       | Low-poly puppet, arms slightly out, knees slightly bent (static stance)  |
| Backdrop     | Stadium wall + fake crowd cards + ad boards (one reads **AXIOM**)        |
| Shadows      | Flat translucent blob quads under kicker, ball, goalie                   |

Team colors: kicker = blue jersey / white shorts / dark socks-boots; goalie =
yellow jersey / black shorts / blue gloves.

Each object is one flat-shaded primitive (`Box`, `FacetedBall`, `Quad`, or
`Line`) with a world position, size, a named **material** (Pass 3 — colors live
in the palette, not on the object), and a stable greppable label (e.g.
`kicker.torso`, `goal.crossbar`, `ad.board.axiom`). Objects are emitted into an
explicit `Vec` in a fixed order and given sequential, stable `ObjectId`s. The
draw layer and flat shading are applied later by the render plan.

> **Pass 3 note.** Colors are now resolved through the named material palette and
> flat-shaded by a deterministic light model; blob shadows are richer per-actor
> ellipses; and the app carries a retro 32-bit visual-style descriptor. See
> `PASS_3_LIGHTING_AND_STYLE.md`.

## Camera placement

A single fixed pinhole camera parked behind the kicker and slightly elevated,
looking down `-Z` toward the goal:

- `eye = (0, 3.6, 16.8)`, `target = (0, 1.15, 2.0)`, `fovY = 46°`,
  `near/far = 0.1 / 120`, `aspect = 16:9`.

The composition shows ball, kicker, goalie, goal, and net clearly. There are no
camera controls in Stage 1.

## Render ordering

> **Superseded by Pass 2.** Ordering now lives in the `penalty_render_plan`
> module and is documented in `PASS_2_DEPTH_ORDERING.md`. The render plan is a
> total, reproducible sort of the draw list (world objects + HUD) by a
> `PenaltySortKey` of `(PenaltyDrawLayer, coarse depth bucket, stable object
> ordinal)` across 14 explicit back-to-front layers:
>
> ```
> Background → Crowd → StadiumWall → RearField → FieldLines → RearNet
>   → GoalFrame → ActorShadow → Goalie → Ball → Kicker → FrontNet
>   → ForegroundEffects → Hud
> ```
>
> The rear/front net split places net geometry behind and in front of the
> actors for retro 32-bit-style fake depth. The ordinal tie-breaker makes the order total
> and independent of any unordered container.

## HUD elements

Chunky, arcade-style, described purely as data (no drawing). Score/round/best are
static; **Pass 4** makes the power meter, aim reticle, and instruction reflect the
live interaction state (`PenaltyHudModel::from_state`), still unlit and rendered
last.

- **Score** panel (top-left): `SCORE 1250` (static)
- **Round** panel (top-center): `ROUND 3 / 5` (static)
- **Best** panel (top-right): `BEST 2520` (static)
- **Shot power meter** (bottom-left): 10 segments; `POWER` fills while charging,
  label flips to `LOCKED` on release (Pass 4)
- **Aim reticle**: mapped from target space over the goal; moves with aim (Pass 4)
- **Instruction** panel (bottom-right): `AIM` / `HOLD` / `RELEASE` per phase (Pass 4)

## What is fake / static in Stage 1

- The net is line geometry only — it does not drape or simulate.
- The crowd is flat tinted cards; ad boards are flat colored boxes (text is a
  label, not rendered glyphs yet).
- Blob shadows are flat translucent quads, not lit/projected shadows.
- Lighting is described as data (one directional + ambient); shading is not
  computed here.
- The puppets are static primitive boxes — no rig, no pose, no motion.
- The render plan is backend-neutral app data; it is **not** yet bound to
  `axiom-scene` / `axiom-render` / `axiom-webgpu`, and nothing reaches a GPU.

## Future stages

1. **Depth sorting & net layering** — ✅ **done in Pass 2**
   (`PASS_2_DEPTH_ORDERING.md`): explicit 14-layer ordering, a
   `(layer, depth bucket, ordinal)` sort key, and the rear/front net split.
   Binding the object list to `axiom-scene` and the render plan to
   `axiom-render` / `axiom-webgpu` for on-GPU presentation remains future work.
2. **Lighting & blob shadows** — ✅ **done in Pass 3**
   (`PASS_3_LIGHTING_AND_STYLE.md`): a deterministic flat-shading light model
   with quantized brightness bands, a named material palette, faked blob
   shadows, an unlit HUD, and a retro 32-bit visual-style descriptor.
3. **Aim & shot-meter interaction** — ✅ **done in Pass 4**
   (`PASS_4_AIM_AND_POWER.md`): a deterministic `PenaltyInputIntent` +
   fixed-tick `Aiming`/`Charging`/`LockedPreview` state machine moves the aim
   reticle in the goal-mouth rectangle and charges the power meter, freezing a
   `PenaltyShotPreview` on release. The ball still does not move.
4. **Deterministic ball arc** — ✅ **done in Pass 5**
   (`PASS_5_BALL_TRAJECTORY.md`): a fixed parametric arc (`sin_pi_approx`) from
   the penalty spot to the mapped goal-plane target, with `BallInFlight` /
   `ArrivedAtGoalPlane` states, a tracking blob shadow, and a trail. No
   goal/save/miss/post resolution yet.
5. **Goalie save volumes** — ✅ **done in Pass 6**
   (`PASS_6_GOALIE_SAVE_VOLUMES.md`): fixed `LeftHand`/`RightHand`/`Torso`/`Body`
   volumes with priority-ordered ball-sphere contact detection, a
   `ContactDetected` state, and off-by-default debug visualization. Neutral
   contact facts only — no save/goal/miss/post resolution.
6. **Goalie puppet pose clips** — ✅ **done in Pass 7**
   (`PASS_7_GOALIE_POSE_CLIPS.md`): the goalie is a 16-part articulated primitive
   puppet that plays one of five authored dive clips (chosen deterministically
   from the locked target), with the Pass 6 save volumes attached to the
   animated hands/torso/pelvis. Still no save/goal/miss/post resolution.
7. **Save / goal / miss / post resolution** — ✅ **done in Pass 8**
   (`PASS_8_SHOT_RESULT_RESOLUTION.md`): a deterministic `Goal`/`Save`/`Miss`/
   `Post` classifier (priority: goalie contact → post/crossbar → goal mouth →
   miss) with a `Resolved` state and result HUD. Score/round/best still static.
8. **Net wobble & impact effects** — net reaction and simple impact feedback.
9. **Scoring & replay polish** — ✅ **scoring/loop done in Pass 9**
   (`PASS_9_SCORING_AND_LOOP.md`): deterministic points (base + power/placement/
   streak bonuses), a 5-round session with continue/reset, round history, an
   app-local best score, and a `SessionComplete` summary.
10. **Impact polish & result juice** — ✅ **done in Pass 10**
    (`PASS_10_IMPACT_POLISH.md`): deterministic net wobble (Goal), post/crossbar
    shake (Post), save impact flash + fake ball deflection, miss drift, crowd
    reaction, additive camera juice, and an animated result banner + score popup
    — all tick-driven descriptors that never change the result or score.
    Persistence / audio / richer blending remain out of scope.
