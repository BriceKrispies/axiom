# Slice 2 — FX: drawing what the FX system already computes

**Files written**

| file | what |
|---|---|
| `apps/shmup/src/scene/wiring/fx_draw.rs` | new — the whole draw seam |
| `apps/shmup/src/fx/decals.rs` | `DecalPlacement` + `DecalSystem::placements()` |
| `apps/shmup/src/fx/particles.rs` | `ParticleLayer::roll_at` |

Nothing was built, checked, tested or formatted, per the manifest. Nothing was
committed. `scene/app.rs`, `scene/game.rs`, `scene/mod.rs`, `scene/wiring/mod.rs`,
`lib.rs`, `Cargo.toml` and `app.toml` were **not** touched; the exact lines they
need are in the report.

---

## What is now drawn

Particles (all five layers), decals, brass, and one pooled flash light.
Tracers, explosions and the muzzle flash needed no code of their own:
`fx/tracers.rs`, `fx/explosions.rs` and `fx/muzzle.rs` are *particle recipes* —
they emit into the same `lit` / `add` / `motes` / `view_lit` / `view_add` layers
everything else does — so drawing the layers draws them. That was worth checking
before writing anything, and `ax def spawn_tracer` / `ax q "emit_add|lights.flash"`
is what checked it.

The one exception is the muzzle flash's *light*, which goes to `fx.lights`
(`muzzle.rs:437`), and the flash's *particles*, which go to the **view** layers:
`on_weapon_fire` sets `view = first_person` (`system.rs:835`) and the player's own
shots are always within 1.5 m of the camera. `FxAudio::particle_points` skips the
view layers on the grounds that "nothing attaches the view scene" — so a renderer
built on that readback would have silently dropped the entire muzzle flash.
`fx_draw` reads the layers directly and composes view-space points through the
camera transform, which is correct for this port because `drive_viewmodel` already
puts the viewmodel in world space.

---

## The finding that dwarfs the slice: **nothing feeds FX any events**

`ax q "\.weapon_fire\(|\.bullet_impact\(|\.weapon_shell\(|\.explosion\("` over
`apps/shmup` returns **zero call sites outside `fx/system.rs` itself**.

`FxAudio` exposes exactly the four forwarders the game needs — `weapon_fire`,
`bullet_impact`, `weapon_shell`, `explosion` — and no one calls any of them.
`Game::frame` even carries the comment *"The weapons before the fx that consume
their events"* directly above the two calls, and the second one does not consume
them: `WeaponsFrame` carries `fire: Option<FirePayload>` and
`shell: Option<ShellPayload>` and they are stored in `self.weapons_frame` and
read by nobody.

So the FX system's only live inputs today are `MovementPulse` (footsteps and
landings) and the ambience. **A perfect renderer, wired today, draws drifting
motes, footstep dust and landing puffs — and nothing else.** No muzzle flash, no
sparks, no tracers, no brass, no decals, no flash light.

This is the same defect shape the manifest names: the port is complete and the
wiring is a thinner substitute that discards the output. Here the substitute
discards the *input*.

Two of the four events are a short bridge in `Game::frame`, given in the report
below (`game.rs` is a shared file, so it is a paste, not an edit):

* **`weapon:fire`** — `WeaponsFrame::fire` is right there. This is what lights
  the muzzle flash and the flash light.
* **`weapon:shell`** — `WeaponsFrame::shell` likewise. This is the brass.

The other two are **not** a bridge; the chain below them is unbuilt, and this is
the honest boundary:

* **`bullet:impact`** — sparks, debris, blood, scorch and *every decal*. An
  impact is produced by `weapons::ballistics`, which raises it through
  `RaycastWorld::fire_bullet` — a seam **nothing implements**
  (`ax q "impl .*WeaponPhysics"` → the trait declaration and no impl), and
  `Game::fixed_update` passes `self.weapons.fixed_step(FIXED_DT, None)`. So no
  round ever hits anything. Making impacts real means implementing
  `weapons::system::WeaponPhysics` for `physics::probe::PhysicsWorld` (which
  already implements four other seams, including `FxWorld` and `DecalWorld`) and
  surfacing the hit out of `WeaponCore`. That is a weapons-tier job, not a
  render one, and inventing a hit here would be fabrication.
* **`explosion`** — nothing in the port raises one at all; there are no
  grenades wired.

**Until at least the fire/shell bridge lands, this slice is untestable end to
end** — the pool will be spawned, hidden, and correct, and the screen will look
almost the same (drifting motes and footstep dust are the only visible change).
That is stated here so nobody concludes the draw path is broken.

---

## Why the pool is a *grid*, not an array

Three engine facts, all checked rather than assumed:

1. **`Transform`, `Bounds` and `Visible` are the only components.** `ax q "impl
   Component for" --path modules/axiom/src` returns exactly three impls. In
   particular there is **no way to change a spawned node's material**, and
   `axiom-scene` has no per-node colour (`ax q "pub fn set_renderable"`). A
   particle's colour and alpha therefore have to be chosen from a **palette
   fixed at install**, and the pool has to be *partitioned by that palette*.
   Everything else in the design follows from that one sentence.
2. **Alpha is `albedo.a * material.opacity`.** `scene_wgsl.rs:626-652` takes
   `surface.opacity` whenever `in.color.w < 1`, and the default surface program
   defines `out.opacity = in.albedo.w` (`wgsl_template.rs`). So a **soft, round**
   particle is reachable: bind the baked atlas tile, whose alpha channel is the
   painter's coverage, and keep the material's own opacity below one. This is the
   difference between particles and hard squares, and it is why no alpha tier is
   `1.0` — an opaque material deliberately ignores the map's alpha.
3. **There is no additive blend reachable from an app.** Additive layers are
   drawn as *emissive* alpha-blended quads. `emissive` is a genuine per-instance
   lane (`FrameDrawItem::with_emissive` → `frame_packet_adapter`'s
   `emissive(3) + specular(1)`), so a spark self-illuminates and blooms.

### The budgets, and the drop policy

| pool | nodes | materials (= max draw calls) |
|---|---|---|
| particles | 292/tier × 4 tiers = **1,168** | 36 |
| decals | 88/tier × 3 tiers = **264** | 18 |
| shells | **14** (= `shells::CAPACITY`) | 1 |
| flash lights | **3** | — |
| | **1,449 nodes** | **55** |

Meshes: **1** (a unit quad; brass reuses `Mesh::cylinder`). Textures: **15**
tiles cut out of the two baked atlases, 256² RGBA8 each, ≈3.9 MB.

The FX simulation at the `Ultra` preset holds ~23,000 particle slots and 512
decal slots, so the draw pool is about **5% of the simulation**. That is
deliberate: the ring buffers are sized for a worst case, and a frame draws what
is alive.

**When a cell is full the sprite is dropped for that frame.** Not from the
simulation, which keeps integrating it; not permanently. Three properties make
that a budget rather than a leak:

* **It is counted.** `FxDrawReport::dropped_particles`, `dropped_decals` and
  `peak_cell_pressure` (the fullest cell, per mille). A frame that drops says so.
* **It does not flicker.** Cells fill in a fixed order — layer order, then
  ascending ring slot — so the same sprites win the same cells every frame. An
  overfull cell loses a *stable* tail.
* **It degrades where it shows least.** The tail of a cell is the highest ring
  slots, i.e. the most recently emitted particles of that class.

Sprites below an alpha floor (`0.08` particles, `0.15` decals) are culled before
they can take a slot — half the lowest tier, below which quantising *up* would
make a dying particle brighter than it is.

---

## Fidelity this cannot reach

Stated rather than faked, per rule 7.

* **Colour is quantised** to nine appearance classes × four alpha tiers (decals:
  six × three). A particle whose spawn colour is far from its class tint is drawn
  in the class tint. The only way to do better is more materials, which is more
  draw calls *and* a more fragmented pool — the grid is the trade, not an
  oversight.
* **An emissive class loses the atlas tile's RGB detail field.** Its albedo is
  black so the tile contributes coverage only. The source multiplies the tile RGB
  into the additive colour; here the emissive term is flat. Fixing it needs a
  per-fragment emissive modulation the fixed-material path does not have.
* **Velocity stretch is dropped.** `ParticleSpawn::stretch` is a screen-space
  smear the source applies to the quad's corners in clip space. There are no
  clip-space corners in a pooled world-space quad.
* **Decals are flat.** The clipped triangle soup — the whole reason
  `fx/decals.rs` exists, 540 lines of Sutherland–Hodgman against the physics BVH
  — cannot reach a pixel: mesh geometry is upload-once and there is no per-frame
  geometry update. What is drawn is the **projector's own face quad**, which is
  exactly the fallback the source itself lays down when the BVH has no triangles
  under the impact (`decals.js:334-357`). **Decals will not wrap a corner.**
* **The decal fade curve is a reading, not a port.** The source's fade lives in a
  GLSL string that was never ported; what *was* ported is the four lanes it reads
  (`birth, 1/life, fade, opacity`). `fx_draw` holds `opacity` until the normalised
  age reaches `fade`, then ramps linearly to zero. One line, marked in the code.
* **A flash light cannot decay smoothly.** `PointLight` is a `Bundle`, not a
  `Component`, so intensity is frozen at spawn. `FX_LIGHT_TIERS` is three
  pre-spawned lights at three fixed engine intensities and the frame lights the
  one nearest the brightest live pooled slot, parking the others at `y = -1000`
  (where the engine's never-zero `1/(1+0.09d+0.032d²)` falloff contributes under
  1e-6). That is a three-step ramp for **one** flash at a time; `fx.lights` has
  four slots and an explosion clamps to the top tier. The light's **colour** is
  frozen too, so the pooled slot's `(r, g, b)` is replaced by one warm white.
* **Sorting within a batch.** `axiom_render::draw_order` sorts translucent draws
  far→near, but a batch of N quads sharing one material collapses to one
  instanced draw. Ordering *inside* that draw is instance-buffer order, which is
  ring-slot order, not depth order. Overlapping sprites of the same class can
  blend in the wrong order. No app-tier fix exists.

---

## Two engine defects found on the way

Neither was fixed — both are outside this slice and outside `apps/`.

### 1. Every runtime material in the process shares one parameter block

`SurfaceKind::code()` deliberately excludes `MaterialParams` from the digest
("two runtime materials with entirely different parameters are one program and
one pipeline, differing only in the bytes below" —
`crates/axiom-surface/src/surface_kind.rs`). But the program cache is **keyed on
that digest** (`modules/axiom-gpu-backend/src/surface_program/cache.rs`:
`program_id: SurfaceProgramPlan::of(surface).program_id()`), and
`SurfaceProgramSource` carries its `params` bytes *inside the cached entry*. So
"the bytes below" have nowhere per-material to live: the first runtime material
prepared wins, and every other one renders with its parameters.

`scene/app.rs` states the opposite in a comment — *"a runtime material's
parameters are excluded from its digest, so 46 parameter sets share one
program"* — and installs 46 per-key surfaces on that basis. If the cache really
is last-writer-wins, **the street's 46 material parameter sets are all rendering
as one**, which would be a large and currently invisible visual defect.

This is why `fx_draw` does **not** use a runtime surface with `alpha_mask: true`,
which is otherwise the tidier route to a soft particle: a particle surface would
race the street for the same block. The built-in fixed-material path has no such
coupling. Someone should verify the cache's parameter binding; if it is per-draw
after all, the comment above is wrong and this note is the thing to correct.

### 2. `CAP_ALPHAMASK` is a frame capability, not a material flag

`scene_wgsl.rs:589` `discard`s any texel with `albedo.a < 0.5` on **every**
textured draw when the bit is set. The shmup street's bake packs its height field
in albedo alpha, so switching it on to get cut-out particles would punch holes in
the whole level. `fx_draw` never asks for it.

---

## What `ax` could not answer

Nothing. Every lookup this slice needed — `ax def Visible`, `ax refs DecalWorld`,
`ax q "impl Component for"`, `ax q "\.weapon_fire\("` (the zero-result search that
produced the finding above) — landed. No friction logged.
