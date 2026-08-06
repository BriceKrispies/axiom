# Champion archive — burnt-rubber

Every champion image that has **landed on `main`**, kept forever so the campaign's
progress is visible without digging through git. `champion.png` one directory up is
overwritten every pass; these are not.

One row per landing, oldest first. `contact-sheet.png` is the same set tiled left to
right behind the current reference — regenerate it whenever a row is added (recipe in
`.claude/skills/visual-convergence/SKILL.md` § Champion archive).

**Reference era** matters: a champion is only comparable to the reference it was scored
against. When the user supplies a new reference the era letter advances, and scores do
not carry across it.

- **era A** — `b8010795`: stylized night straight, green conifers, lit apartment blocks,
  a bright red car, blue horizon glow. Retired 2026-08-04.
- **era B** — `493dac58`: near-black night, no visible shoulders or skyline, a
  black muscle car with twin stripes and glowing tail-light bars on wet glossy
  asphalt. Retired 2026-08-06.
- **era C** — current: full daylight on a coastal highway — sun and lens flare,
  cumulus, turquoise sea with sailboats, sand, palms casting hard shadows across
  the road, stilt houses, mountains, a scarlet wedge coupe with twin white
  stripes at 162 km/h. The user asked for this knowing it inverts eras A and B.
  The **moment** moved with it: eras A and B were parked on the opening straight,
  era C is driving through COASTAL SWEEPERS. **Judged on the GPU/WebGL arm only**
  (see `../campaign.toml`).

| # | Landed | Commit | Era | What landed | Lowest axis after |
|---|---|---|---|---|---|
| 0000 | 2026-08-01 | `b8010795` | A | Campaign seed — first live-browser capture at the reference moment. | `atmosphere` 0 |
| 0001 | 2026-08-01 | `fc940ada` | A | Pass 1: art-director start grid, modeler fastback rear, surfacing tail lights, lighting night key/fill, architect forwards the authored render look to the live arm. | `atmosphere` 0 |
| 0002 | 2026-08-01 | `8261e072` | A | Pass 2: emissive reaches the pixels, greenhouse chop, chase rig 15% closer, pool light, asphalt aggregate grain. | `atmosphere` 0 |
| 0003 | 2026-08-01 | `fac33fc4` | A | Pass 3: prop-kind draw distances, chase eye at roof height, key aimed so its shadow lands in frame, asphalt grain tiled in world metres. | `contrast_and_exposure` 0 |
| 0004 | 2026-08-04 | `main` | B | No convergence pass. Re-capture of `main` under the **new** era-B reference, after nine unrelated commits across the app and the render spine (start screen, phone rails, lane lattice, collision episodes, and the two large ones — `08731280`'s FrameSky/FrameBloom/GPU post chain and `ece53937`'s material textures + mip chains) had moved the render out from under the era-A champion. Baseline for era B. | `contrast_and_exposure` 0 |
| 0005 | 2026-08-04 | `d2509552` | B | Era-B pass 1, all seven lenses: chase rig scaled to roofline height, twin stripes + number plate on the car, both global light terms cut, asphalt grain rescaled to aggregate size, sky black level dropped ~13x, and the spine fix that finally carries the colour grade to the live browser arm (plus a black-point term the chain had no way to express). Attacked `contrast_and_exposure` 0 → 2: the scene band went from 2.4% of pixels below L=16 to 83.7%, against the reference's 80.2%. Overshot into a clipped floor — `low_key()`'s 0.16 black point is the one constant the next pass retunes. | `silhouette_readability` 1 |
| 0006 | 2026-08-05 | `03f15dca` | B | Era-B pass 2, all seven lenses: chase rig set off the tail-light bar (the one ruler both frames share), tail lamps become twin-tube clusters in fixed bezels plus a centre badge, the key raised so the ground plane clears the grade's hard black-point clip, asphalt grain moved from cell scale to texel scale, tarmac hue rotated off blue at matched luminance, and a spine supersample tier giving the render-scale path an upward direction. Attacked `subject_fidelity` 1 → 2; four axes rose, none fell. The pass-1 clipped floor is repaired — road `(0,2,11)` → `(14,14,17)` against the reference's `(7,8,11)`. `artifact_level` held as **unverified**: the supersample's effect could not be separated from the road's simultaneous luminance rise. | `silhouette_readability` 1 |
| 0007 | 2026-08-06 | `c8d11623` | C | No convergence pass. Re-capture of `main` at the **new era-C moment** (COASTAL SWEEPERS, placed and frozen with the browser probe) under the daylight reference. Baseline for era C. The app is unchanged from 0006 apart from the day's gameplay work — camera pulled back, near-miss rule, canvas2d paint window, one-tap-one-lane fix — none of which is a look change. | `lighting_and_shadow` 0 |
| 0008 | 2026-08-06 | `8f027579` | C | Era-C pass 1 — the night-to-day conversion. Six lenses stacked (one declined by the human): daylight sky/sun/grade, a daylight light rig, a spine cloud layer in `FrameSky`, a palm avenue, asphalt grain cut to the reference's measured amplitude. Attacked `lighting_and_shadow`. Overshot bright, and the cloud layer buried the sky gradient. | `artifact_level` 0 |
| 0009 | 2026-08-06 | `b0f4e518` | C | Era-C pass 2 — **the anamorphic squash is fixed**. `axiom-windowing` now measures the canvas instead of declaring a compile-time size, so buffer and CSS box agree to four decimals at DPR 2; the rig was re-solved with it (arm 5.60 → 8.91 m). Plus the gloss set re-sized for the daylight key, the key solved off the reference's own tarmac, a daylight bloom preset, and a roadside shrub band. `scale_and_proportion` scored 1 → 3 at landing, **recalibrated to 2** by the critic pass (ledger iteration 15): the reference-car width the ceiling rested on (`0.598w`) is wrong by 2x — the reference's car is ~`0.30w` and the champion's is `0.313w`, so there is no width gap, and the subject's aspect is still off 2-3x. Three further axes folded in the same recalibration (`silhouette_readability`, `material_and_texture_detail`, `lighting_and_shadow`, each 2 → 1); `final_score` 1.20 → 1.10 on an unchanged image. | `subject_fidelity` 1 |
| 0010 | 2026-08-06 | `d054860b` | C | Era-C pass 3, seven lenses, **zero cherry-pick conflicts** — the first clean stack. **The car casts a shadow for the first time in fourteen iterations**: the shadow map was never broken, it was anchored at `Vec3::ZERO` while the moment sits 1.9 km along the course, and the shader treats anything outside the frustum as lit. Plus the cloud lid off (coverage threshold was *below* its own field mean), a glazed rear aperture so the roof stops being glass, a graded hemisphere, and a cross-road grain octave the anisotropic sampler cannot average away. Every axis reached 2; `final_score` 1.10 → 2.00. | 12-way tie at 2 |
