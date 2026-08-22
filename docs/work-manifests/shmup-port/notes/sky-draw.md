# Slice 4 — the sky

**Produced:** `apps/shmup/src/scene/wiring/sky_draw.rs`.
**Edited inside `apps/shmup/src/sky/`:** nothing. See "what I did not do".

## The finding, first

The engine already has a sky pass, and this app was not using it.
`axiom_host::FrameSky` (reachable as `RunningApp::set_sky` and
`WindowingApi::set_sky`, both already in the app's prelude) evaluates a
two-stop vertical gradient with an authored haze band, one celestial body with a
disc and a halo, and a procedural cloud layer — per pixel, behind the scene,
gated by `RenderCapability::Sky`. `apps/shmup` authored none of it and cleared to
a flat colour.

So the slice's answer to "check what the engine's own sky support already
accepts, and if the engine's sky pass already covers the dome, use it and say so"
is: **it does, and this uses it.** No dome mesh was built.

## Why not a dome mesh — the decision, in full

The port carries a complete dome shader (`sky/dome.rs::sample`: sky-view LUT
lookup, two aureoles, two cloud decks, the night sky, ground bounce, horizon
murk, highlight roll-off, sun disc, moon disc). Drawing it means baking an
equirect texture and hanging it on an unlit dome mesh. That was rejected for
three independent reasons, any one of which is sufficient:

1. **They are mutually exclusive.** A dome drawn as app geometry occludes the
   engine's sky pass. Picking the mesh means giving up the capability gate, the
   depth-fog horizon match, and the backend-neutral degradation.
2. **`axiom_host::frame_sky`'s own module doc argues this exact case**: cloud
   (and by extension the sky) "belongs here, in the sky's own evaluation, and not
   in the app as billboard cards", because app-tier sky geometry "would survive
   on a backend that has already declared it drops `RenderCapability::Sky`, which
   is precisely the silent divergence the capability system exists to prevent."
   A dome mesh is that divergence, one tier larger.
3. **Cost.** `dome::sample` is a CPU reference implementation of a fragment
   shader. Feeding it needs the 384x192 sky-view LUT bake `look.rs` deliberately
   skipped (73,728 raymarches x 40 steps), and then one `sample` per equirect
   texel — each running a six-octave cumulus fbm, a two-octave cirrus ridge, a
   five-octave Milky Way and two disc evaluations. That is a multi-second startup
   freeze in wasm, to reproduce on the CPU what the GPU sky pass already does.

A dome mesh would also be lit, depth-fogged and far-plane-clipped like any other
geometry. The engine's sky is none of those.

## What was wired

`visible_sky(&SkyDriver) -> FrameSky`. It reads the driver the *lighting* path
already owns — nothing here constructs a second `SkySystem`, and the lighting
path is untouched. Every number is measured off the port or read out of its
published state; none is hand-picked.

| `FrameSky` knob | where the number comes from |
|---|---|
| `zenith` | `SkyDriver::radiance().ambient_sky` — the driver's existing raymarch straight up. Not recomputed. |
| `horizon` | `SkyDriver::radiance().clear_color` — the driver's existing raymarch at 12 degrees on the anti-solar bearing. This is deliberately the *same* value as the window clear colour and the depth-fog target, so the horizon cannot seam. |
| `haze_height` | **Measured.** 24 raymarches up the anti-solar column, scanning for the elevation at which displayed luminance stands halfway between the two stops. That elevation *is* the haze height exactly — `smoothstep(haze_lift(up,h)) = 0.5` only when `h == up` — so this is a solve, not a fit. |
| body direction / angular radius | `SkySystem`'s ephemeris and `shared.disc`. The radius is the **drawn** limb `uDisc.x * uDisc.z`, matching `dome::sun_disc`'s own `r_edge`. |
| body colour | `shared.sun_disc_radiance * T(view->sun) / draw_scale^2` — `dome::sun_disc`'s chain evaluated at the disc centre, then through `look::display`. |
| `halo_falloff` / `halo_strength` | **Fitted in closed form** to the port's own `dome::aureole`, probed at 2 and 12 degrees (inside `dome`'s 24-degree `AUREOLE_CUT`). Two probes, two unknowns, one log divide. Guarded to `(1.0, 0.0)` when the probes carry no slope. |
| `cloud_coverage` | `SkySystem::weather.cloud_coverage`, pass-through. Justified: the port scales its authored coverage by `0.34 + 1.30 * cloud_macro`, whose macro field averages 0.5, so the mean effective coverage over the deck *is* the authored number. |
| `cloud_scale` | **Derived** from the deck: `PI * CUMULUS_KM * CUMULUS_FBM_FREQUENCY` = 5.89. The engine's field is a sinusoid sum whose lobes tile on a `PI` pitch; the port's cumulus samples value-noise at 1.25/km on a deck 1.5 km up. Equating the two feature spacings in the shared tangent coordinate gives this. |

The key body is whichever of sun/moon `SkySystem::key_light` resolved, so the
visible sky and the key light can never disagree about which body is up.

Cost at build: ~26 short raymarches. The expensive LUT bakes stay in
`SkyDriver::new` and are **not** repeated — which is why this needs the accessor
below rather than baking its own copy.

## What could not cross the boundary

Named rather than approximated, per rule 7.

* **Stars and the Milky Way (`sky/stars.rs`, 197 lines) — no engine counterpart.**
  `FrameSky` has no star term and there is no other seam that accepts one.
  `Material::with_emissive` takes a flat `Color`, not a map, so even a night-sky
  mesh would need an `axiom_surface::LightingModel::Unlit` surface plus a baked
  equirect albedo — the dome-mesh path already rejected, taken for the one layer
  that is invisible at this level's authored hour (`look::HOUR` is 9.5,
  mid-morning). `sky/stars.rs` therefore stays unreferenced. **The honest fix is
  an engine one**: a star/night term on `FrameSky`, evaluated by the sky pass
  alongside the cloud layer it already carries. That is a new engine capability,
  which this wave forbids.
* **Volumetrics (`sky/volumetrics.rs`, 855 lines) — no reachable engine seam.**
  `axiom_host::FrameVolumetrics` exists (a screen-space god-ray post-pass, gated
  by `RenderCapability::Volumetrics`), but **no app-tier setter reaches it**:
  `FramePacket::with_volumetrics` is host-internal, and neither
  `axiom::RunningApp` nor `axiom_windowing::WindowingApi` exposes a
  `set_volumetrics` the way both expose `set_sky`, `set_bloom`, `set_depth_fog`,
  `set_ambient`, `set_grade` and `set_tonemap`. That asymmetry looks like an
  oversight in the render-look bundle rather than a decision, and is worth
  raising on its own. Even with the setter, the shapes do not meet: the engine's
  is a radial screen-space blur around a light position; the port's is a
  half-resolution raymarch through a height-fogged, phase-functioned medium,
  sampling a cascaded shadow atlas per step and temporally resolved against a
  history buffer. The engine offers an app no depth-buffer and no shadow-atlas
  seam to march. **Stopped at the boundary; nothing approximated.**
* **The second body.** The port draws sun *and* moon discs and sums *both*
  aureoles. `FrameSky` carries one body. Only the key body's disc and aureole
  cross.
* **The moon's phase.** `dome::moon_disc` is a gnomonic projection with
  procedural maria, a real terminator with regolith backscatter and earthshine.
  `FrameSky`'s body is a limb-softened uniform disc. The moon renders as a plain
  disc.
* **The cirrus deck (7.8 km).** `FrameSky` has one cloud layer; the cumulus deck
  takes it. `weather.cirrus_coverage` / `cirrus_opacity` have nowhere to go.
* **Wind.** The port advects both decks by `weather.wind_speed`/`wind_angle`;
  the engine's cloud field takes no time input. The clouds do not drift.
* **The azimuthal term.** The port's sky-view LUT is 2D (azimuth x altitude);
  `FrameSky`'s gradient is azimuth-invariant. The two stops are measured on the
  anti-solar column — the band the camera actually looks into — so the loss lands
  on the solar side of the sky rather than on the band that fills the frame.
* **Horizon murk and the highlight roll-off** (`dome::sample`'s last two terms
  before the discs) fold into the measured horizon stop and are not separately
  representable.

## An honest note on colour space

The two gradient stops and the body colour go through `look::display` — the
Reinhard stand-in `look.rs` labels "invented, and labelled as such", the same
transform the window's clear colour already uses. That keeps the sky in exactly
the exposure the frame is in today and adds only shape. Its cost is that the sun
disc saturates near white and therefore cannot bloom: the app sets
`FrameTonemap::filmic()` and an `Rgba16Float` scene target, so there is real HDR
headroom above the disc that a display-referred sky cannot reach. Re-basing the
sky onto scene-referred radiance with a real exposure would move the clear
colour, the ambient and the fog target with it — that is the *lighting* path,
which this slice was told not to disturb. Flagged, not fixed.

## What I did not do, and why

* **No file inside `apps/shmup/src/sky/` was edited.** Nothing in the ported sky
  was wrong for this purpose; every value the engine's sky pass needs was already
  published. The unreferenced symbols that remain (`stars`, `volumetrics`, most
  of `dome`, the cirrus half of `clouds`, `fullscreen`) are unreferenced because
  the engine has nowhere to put them, not because the wiring missed them.
* **`scene/wiring/look.rs` was not edited**, although `visible_sky` needs one
  additive accessor on `SkyDriver`. Slice 5 (`weapon_look.rs`) may also be
  reading `look.rs`, and a concurrent read-modify-write in a shared checkout
  loses one side's edit. The accessor is in the report as a paste-ready block.
* **`scene/wiring/mod.rs` was not edited** — every slice adds a line to it, so it
  is a collision surface. The line is in the report.
* **Nothing was built, checked, tested, linted or formatted**, and no mutating
  git command was run.

## Deletions

None. There is no thinner hand-inlined substitute for the visible sky in this
app — there was no visible sky at all. `SkyDriver` is the real port's seam and it
stays exactly as it is; this module reads it and adds nothing beside it.

`ax refs SkySystem` gains no new consumer (this reads the driver, not the facade
directly); `ax refs SkyDriver` gains one outside `look.rs`, and
`ax refs aureole`, `ax refs CUMULUS_KM` and `ax refs display` each gain their
first consumer outside their own file.
