# Pass 3 — Deterministic Lighting, Blob Shadows & retro 32-bit-Style Treatment

Passes 1–2 built a statically-composed, deterministically-ordered diorama out of
flat-colored primitives. Pass 3 makes it look *intentionally* low-poly/retro 32-bit-ish
rather than merely primitive: a fixed flat-shading light model, a named material
palette, faked blob shadows, and a retro 32-bit visual-style descriptor.

Still a static, fixed-camera visual pass — no gameplay, no motion.

## What Pass 3 adds

- **`PenaltyLightModel`** (`penalty_light.rs`) — one directional + one ambient
  light, brightness quantized into fixed bands. A pure function from a face
  normal to a shaded color.
- **Material palette** (`penalty_materials.rs`) — every object now references a
  named `PenaltyMaterialId` instead of a raw color; the ordered `PENALTY_PALETTE`
  array is the single source of truth for colors.
- **`PenaltyVisualStyle`** (`penalty_style.rs`) — the retro 32-bit style descriptor
  (internal resolution, pixel snapping, flat shading, brightness quantization,
  nearest filtering; no PBR, no dynamic shadows).
- **`PenaltyBlobShadow`** (`penalty_blob_shadow.rs`) — three fixed blob-shadow
  descriptors (kicker/ball/goalie).
- **`PenaltyStylePass`** (`penalty_style_pass.rs`) — bundles the light model +
  style; the render plan uses it to flat-shade world materials.

Objects lost their raw `color` field; the render plan resolves each material and
attaches a representative flat-shaded color and a `lit` flag to every world
render item. HUD items carry their materials verbatim and are marked unlit.

## The deterministic light model

```text
face_brightness(normal) = ambient + max(dot(normal, -light_dir), 0) * directional
shade(base, normal)     = base.rgb * quantize(face_brightness(normal))   // alpha kept
```

Fixed constants (documented, never read from the clock, never random, never from
a browser API):

| Constant              | Value                                             |
|-----------------------|---------------------------------------------------|
| ambient strength      | `0.35`                                            |
| directional strength  | `0.65`                                            |
| light direction (raw) | `(-0.45, -1.0, -0.35)` (upper-front-left)         |
| light direction (unit)| `(-0.390932, -0.868799, -0.304059)` (precomputed) |

The direction is stored pre-normalized so no runtime normalization (and no
fallible math) is needed. There is no light movement and no shadow mapping.

## Brightness quantization bands

Continuous brightness is snapped **down** to the largest band it meets or
exceeds, floored at the first band:

```
bands = [0.35, 0.50, 0.70, 0.90]

b < 0.50            -> 0.35
0.50 <= b < 0.70    -> 0.50
0.70 <= b < 0.90    -> 0.70
b >= 0.90           -> 0.90
```

Because `face_brightness` ranges over `[0.35 .. 1.0]`, every face lands in one of
four flat bands — the stepped, banded look of retro 32-bit-era shading. A face pointing at
the light is `1.0 -> 0.90`; a face pointing away is `0.35`; an up-facing top face
(under this upper-front-left light) is `~0.915 -> 0.90`.

## The fixed material palette

`PENALTY_PALETTE` is an ordered array (never a map). Each entry is
`{ id, name, base_color, unlit }`, and each id's discriminant equals its array
index, so `material(id)` is a direct index — deterministic and total. The
palette includes every required named material:

field grass · darker grass band · white field lines · goal frame white ·
net off-white · goalie jersey yellow · goalie shorts black · goalie skin ·
goalie hair · kicker jersey blue · kicker shorts white · kicker socks dark ·
ball white · ball dark panels · crowd muted colors · stadium wall dark gray ·
ad board red · HUD dark panel · HUD white text · HUD yellow highlight ·
HUD green success · HUD red warning

plus a few honestly-named extras the scene needs (kicker skin, goalie gloves,
two alternate muted crowd tints, a dark ad board, and the blob-shadow material).
World materials are lit; HUD materials and the blob-shadow material are `unlit`.

## How blob shadows are faked

Each blob shadow is a flat, dark, **translucent** ground quad (the `blob shadow`
material, `unlit`) laid under an actor:

- **ball** — small, directly under the static ball;
- **kicker** — elongated along the field;
- **goalie** — near the goal line, under the goalie.

They are emitted as world objects with `role = BlobShadow`, so the Pass 2
ordering places them in the `ActorShadow` layer — after the field and field
lines, before the goalie/ball/kicker — each with a stable ordinal. They require
no real-time lighting and no shadow maps.

## Why this is not real physics or real shadow rendering

Nothing here simulates light transport. "Shading" is a single dot product per
face quantized into four bands; "shadows" are hand-placed dark ellipses that do
not respond to the light direction, the actors, or each other. This is a
deliberate stylistic fake — cheap, fully deterministic, and replayable — not a
shadow-mapping pipeline, not PBR, and not a postprocessing stack.

## Why the HUD is unlit

The HUD is a 2D arcade overlay, not part of the 3D scene. Lighting it would make
the score/round/best text and the power meter flicker between brightness bands
and lose contrast. HUD render items are marked `lit = false`, carry their
materials verbatim (dark panel + a fixed highlight color per element), and always
render last (the `Hud` layer), so the HUD stays crisp and readable.

## How this supports the retro 32-bit-style target

- Flat, per-face shading quantized into a few bands → chunky, banded surfaces.
- A small, saturated-but-muted named palette → stable, readable silhouettes.
- A low internal resolution (`426x240`) with pixel snapping and nearest
  filtering → the characteristic retro 32-bit crunch (a renderer/backend applies these;
  the app only declares the intent).
- Fake blob shadows → grounded actors without any shadow pipeline.

Lighting and style are descriptor-only and never touch the sort key, so all Pass
2 deterministic ordering is preserved exactly.

## Still not implemented (later stages)

Pass 3 is lighting/materials/style only. It deliberately does **not** add:

- shooting or shot input;
- a ball trajectory / arc;
- goalie animation or dive poses;
- collision volumes of any kind;
- save / goal / miss / post resolution;
- net wobble or impact effects;
- scoring, rounds, or replay logic.

The camera is still fixed, the net is still static line geometry, and no real
physics or dynamic shadows exist. See `STAGE_1.md` for the full roadmap.
