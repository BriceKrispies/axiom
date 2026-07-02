# Pass 7 — Deterministic Goalie Pose Clips & Animated Save Volumes

Pass 6 gave the *static* goalie invisible save volumes. Pass 7 makes the goalie
an **articulated primitive puppet** that plays one of five authored dive pose
clips based on the locked shot target, and bolts the Pass 6 save volumes onto the
animated hands / torso / pelvis so contact detection now runs against the *posed*
keeper.

Still deterministic, still fixed-camera. **No final save/goal/miss/post result,
no scoring, no ball deflection, no net wobble.**

## What Pass 7 adds

- **`penalty_goalie_pose`** module: `PenaltyGoaliePartKind` (the 16-part
  hierarchy), `PenaltyGoaliePart`, `PenaltyGoaliePose`,
  `PenaltyGoaliePoseDescriptor`, `PenaltyGoaliePoseFrame`,
  `PenaltyGoaliePoseClip`, `PenaltyGoaliePoseSampler`,
  `PenaltyGoalieDiveLane`, `PenaltyGoalieAnimationState`,
  `PenaltyGoalieAnimation`, `PenaltyGoalieClipLibrary`, and
  `PenaltyGoalieAnimatedVolumeSet`.
- The goalie is emitted as the 16-part rig (idle pose) and the app overlays the
  sampled dive pose each frame.
- The interaction state carries a `PenaltyGoalieAnimation`; contact detection
  uses the animated volume set.

## Why the goalie is an articulated primitive puppet, not a skeletal rig

The goalie is a tree of primitive boxes with per-part local [`Transform`]s
composed through the hierarchy. There are no bones, no skin weights, no vertex
blending, no IK solver, and no blend trees — just parent-child transform
composition (`Transform::combine`) over a fixed 16-entry array. That is the
smallest thing that reads as an articulated dive and stays fully deterministic
and testable, without pulling in a skeletal-animation pipeline the app does not
need.

## The stable goalie part hierarchy

Sixteen parts, in fixed ordinal order (parent always precedes child):

```
0  Root
1  Pelvis         parent Root
2  Torso          parent Pelvis
3  Head           parent Torso
4  LeftUpperArm   parent Torso
5  LeftForearm    parent LeftUpperArm
6  LeftHand       parent LeftForearm
7  RightUpperArm  parent Torso
8  RightForearm   parent RightUpperArm
9  RightHand      parent RightForearm
10 LeftThigh      parent Pelvis
11 LeftShin       parent LeftThigh
12 LeftFoot       parent LeftShin
13 RightThigh     parent Pelvis
14 RightShin      parent RightThigh
15 RightFoot      parent RightShin
```

Each part carries a stable ordinal, its parent ordinal, a local + world
[`Transform`], and a box + material descriptor. Runtime access is by
enum/ordinal — never by string lookup (the label map is only used to overlay a
sampled pose onto the emitted objects).

## The five authored dive clips

`DiveLeftLow`, `DiveLeftHigh`, `DiveRightLow`, `DiveRightHigh`, `DiveCenter`.
Each clip is `CLIP_DURATION_TICKS = 24` ticks with five keyframes:

1. **idle** — ready stance (tick 0),
2. **anticipation** — a small crouch (tick 4),
3. **launch** — push off toward the lane (tick 9),
4. **extension** — full reach (tick 16),
5. **settle** — land (tick 24).

Frames are built from the idle pose plus fixed, `m`-scaled per-part offsets: the
`Root` shifts sideways (the dive), the pelvis crouches, and the lead hand + upper
arm reach toward the lane (both hands for center). Transforms are exaggerated and
readable, not realistic.

## The deterministic dive-lane selection table

Selected from the locked normalized target (`x ∈ [-100,100]`, `y ∈ [0,100]`) —
no randomness, no difficulty, no prediction, no probability:

| Condition                | Lane            |
|--------------------------|-----------------|
| `x < -35` and `y < 50`   | `DiveLeftLow`   |
| `x < -35` and `y >= 50`  | `DiveLeftHigh`  |
| `x > 35` and `y < 50`    | `DiveRightLow`  |
| `x > 35` and `y >= 50`   | `DiveRightHigh` |
| otherwise                | `DiveCenter`    |

## The pose sampling model

`PenaltyGoaliePoseSampler::sample(clip, tick)` clamps `tick` to
`[0, duration]` and returns the pose of the last keyframe whose tick `<= tick`
(nearest-previous / hold; no interpolation in Pass 7). Sampling before the start
yields the first pose, after the duration the final pose, and the same
`(clip, tick)` always yields the same pose.

## How animated save volumes attach to puppet parts

`PenaltyGoalieAnimatedVolumeSet::from_descriptor` reads the resolved pose and
places the Pass 6 volumes on the animated parts, keeping the fixed shapes, radii,
and **priority order**:

| Priority | Volume      | Attached to part | Shape  |
|----------|-------------|------------------|--------|
| 0        | `LeftHand`  | `LeftHand`       | sphere |
| 1        | `RightHand` | `RightHand`      | sphere |
| 2        | `Torso`     | `Torso`          | AABB   |
| 3        | `Body`      | `Pelvis`         | AABB   |

Contact detection reuses the Pass 6 `PenaltyGoalieContactDetector` unchanged —
`PenaltyGoalieContactDetector::new(animated_set)` — still treating the ball as a
sphere of the fixed Pass 5 radius, still producing neutral
`Hand`/`Torso`/`Body`/`None` frames, and still freezing the ball into
`ContactDetected` on the first contact tick. It does not deflect the ball or
change the score.

## Animation state model

- **Idle** — while aiming/charging (ball at the spot).
- On lock (release), one dive lane is chosen and the goalie enters
  **TrackingShot** (clip tick 0).
- **Diving** — the dive clip advances one tick per shot tick.
- **Landed** — when the clip reaches its duration.
- `reset` clears the lane and returns to **Idle**.

## Why this is not ragdoll physics

Nothing simulates the body: no forces, no joints solved under gravity, no
collision response, no integration. The pose at any tick is a pure lookup of an
authored keyframe composed through the hierarchy.

## Why this is not skeletal skinning

There is no skeleton driving a skinned mesh: each part is its own rigid primitive
box. Parts move by transform, and the box moves with them — there are no vertex
weights, no bind pose, and no deformation.

## Why this is not a general animation framework

There is one fixed 16-part rig, one clip shape (5 keyframes), one nearest-frame
sampler, and one lane table — all specific to this goalie. There are no generic
tracks, channels, curves, retargeting, or blend graphs, and nothing here is meant
to be reused outside this app.

## Why final save/goal/miss/post resolution is still deferred

Pass 7 only makes the keeper *animate* and reports *where* the ball touched the
posed keeper. Turning that into a `SAVE` / `GOAL` / `MISS` / `POST` still needs
goal-line and post geometry and the scoring rules, which a later pass will layer
on top of the contact frame produced here.

## Still not implemented (later stages)

- final shot result classification (save / goal / miss / post);
- ball deflection off the keeper;
- score / round / best-score changes;
- net wobble or impact effects;
- post-hit reaction;
- crowd / result polish.

See `STAGE_1.md` for the full roadmap.
