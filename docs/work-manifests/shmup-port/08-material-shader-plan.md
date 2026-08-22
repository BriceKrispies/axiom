# The runtime material shader — porting `materials/shader.js` into `gpu-backend`

`C:/dev/Claude-of-Duty/src/materials/shader.js`, 890 lines, is the last large
piece of the visual gap. All nineteen procedural surface generators are ported
and golden-verified, and **nothing samples them**. This is the file that does.

## Where it goes, and why

**`modules/axiom-gpu-backend/src/material_shader/`** — hand-written WGSL, not
the field algebra.

`01-engine-gaps.md` already made the argument and it holds: the field algebra
has no control flow, no loops, no division, no derivatives and no texture
sampling, with a 256-node budget per *whole surface*. The source's runtime
shader needs exactly those things — parallax occlusion mapping is a bounded loop
with a linear refine, de-tiling needs `textureGrad` with explicit derivatives,
triplanar is nine fetches. The algebra's branchlessness is the Branchless Law
itself and is immovable, so the split is not a compromise: it mirrors how the
source is built, a bake to a target and then a runtime shader that samples it.

## The seam already exists

This is the part worth being pleased about. `scene_wgsl.rs` is not a monolith —
it is a prefix, a **program-shaped hole**, and a suffix, spliced by
concatenation in `surface_program::wgsl_template::scene_shader`:

```wgsl
fn axiom_surface(in: SurfaceIn, params: SurfaceParams) -> SurfaceOut
```

Today that hole is filled either by `DEFAULT_SURFACE_WGSL` (pass the albedo
through) or by WGSL *generated* from an authored `axiom_surface::Surface`. The
runtime material shader becomes a **third filling**: hand-written WGSL that
honours the same signature.

Consequences, all good:

- No new pipeline mechanism, no permutation explosion. The existing
  content-addressed program identity already gives one pipeline per distinct
  program.
- Nothing about the lighting maths, the PCF shadow lookup, the hemisphere
  ambient, the fog or the tonemap changes. A surface program supplies channel
  values; it never supplies a way of being lit.
- The field algebra keeps bake time, which is what it is good at.

## What the seam does not yet carry

Four gaps, and each is a deliberate, additive contract change:

1. **World space.** `SurfaceIn` has `object_pos`/`object_normal` only, and says
   why: a world-space pattern swims when the object moves. But this shader's
   weathering is *explicitly* world-anchored — rain runs down, ground splash is
   measured from `groundY`, the dust wedge sits at the wall/ground junction, and
   triplanar projects on world axes. So `SurfaceIn` gains `world_pos`,
   `world_normal` and `view_dir`. Additive: a generated program that ignores
   them is unchanged, which the parity suite will confirm.
2. **Textures.** POM, de-tiling, the detail layer and the macro layer all
   sample. Group 0 currently binds albedo + normal. It gains the packed
   material set (roughness/metalness/AO), the shared detail normal, and the
   macro noise. Group 0 and not a new group: WebGPU guarantees only four bind
   groups and 0–3 are taken.
3. **Parameters.** `DEFAULT_PARAMS` is ~30 knobs. `SurfaceParams` is already
   `array<vec4<f32>, 32>` = 128 floats. It fits with room; the packing is a
   table in `params.rs` and is pinned by a test.
4. **Derivatives.** Fragment-stage only, which is where this runs. No change.

## How it is verified

The repo already has the right instrument and it works here: a **CPU reference
evaluator that is the semantic definition**, plus **CPU↔GPU parity on a real
adapter** with per-operator measured tolerances (`surface_program/parity*.rs`,
20/20 green under `--features offscreen`). A tolerance more than 10× looser than
the hardware needs is itself a failure.

So every layer lands as three things in one change:

- the WGSL,
- a CPU reference for the same maths,
- a parity test proving they agree on a real GPU,

plus, where the source's own JavaScript can be run, a golden captured from it —
the same discipline as the rest of this port. Note the honest limit, already
learned the hard way in `sky/`: where the source is GLSL in a string there is no
oracle, and a transcription written by reading one's own Rust shares its
mistakes. Ten defects were found in `sky/` exactly that way. **Transcribe from
the GLSL text alone.**

## The laws that bind this work

Unlike `apps/shmup`, this is the **spine**:

- **Branchless Law** — the Rust that assembles the WGSL must contain zero
  control flow. Note the WGSL *itself* has loops (POM) and that is fine: the
  `engine_no_branching` dylint reads Rust HIR, and a loop inside a `&str` is not
  Rust control flow. The shader text is data.
- **Coverage Law** — 100% of the new Rust, in the same change.
- **Module Law** — `gpu-backend` is a module; it may not grow a dependency on
  another module.

## Stages, in dependency order

| # | stage | what it touches |
|---|---|---|
| 1 | `SurfaceIn` gains `world_pos`/`world_normal`/`view_dir`; parity suite proves generated programs are unmoved | `wgsl_template.rs`, `scene_wgsl.rs`, `parity*.rs` |
| 2 | Group 0 gains the material texture set; bind-group layout + a neutral default so existing draws are pixel-identical | `scene_renderer.rs`, `texture_sampling.rs` |
| 3 | The parameter block: `DEFAULT_PARAMS` packed into `SurfaceParams`, pinned | `material_shader/params.rs` |
| 4a | uv mode — planar / triplanar / mesh, scale, offset, localSpace | `material_shader/` |
| 4b | POM: bounded loop + linear refine, `parallaxFade`, `parallaxLayers` | |
| 4c | de-tiling — the second sample with explicit `textureGrad` | |
| 4d | detail layer, incl. the `detailWorld` derivation the source documents at length | |
| 4e | macro variation + `macroBig`'s second band + `macroRelief` | |
| 4f | repair patches on vertical faces | |
| 4g | weathering: rain runoff, ground splash, dust wedge | |
| 4h | cavity + vertex-colour masks | |
| 4i | tint, `wearMaterial`, the roughness/AO remap | |
| 4j | the cloth transmission override (`CLOTH_LIGHT`) | |
| 5 | `apps/shmup` authors materials that select it; serve and screenshot | `apps/shmup` |

Stage 5 is the only one outside the spine, and it is the one that turns the
nineteen already-verified generators into something you can see.

## Traps carried over from the app-side port

Every one of these has already cost real time on this port and each applies here:

- **`sign` is not `signum`**, and **GLSL `sign` is not `Math.sign`** either —
  they differ at `-0`. `crate::jsmath` in the app owns the JS one; this is GLSL,
  so transcribe GLSL's.
- **Float arithmetic is not associative.** Do not tidy a weathering blend or a
  POM accumulation. A division written as a reciprocal-multiply is a real
  defect; `sky/` produced ten of them.
- **`Math.hypot` is not `sqrt(x*x + y*y + z*z)`**, and `Vector3.length()` *is* —
  check which the source calls at each site.
- **Storage width is part of the algorithm** — found five times in this port,
  most sharply as an `f32` `rotateY(PI)` carrying a shear of `-8.74e-8` where
  `f64` gives `1.22e-16`.
- **Dead computation in the source is still part of the source.**
- **Run goldens under MSVC.** The default `windows-gnu` toolchain's `cos`/`sin`
  zero the low 40 mantissa bits near axis angles — 3.3e-5 relative, which a
  fractional power amplifies into visible divergence. `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`.

---

# Progress, and the one open design decision

## Landed

| stage | state |
|---|---|
| 1 — `SurfaceIn` gains `world_pos`/`world_normal`/`view_dir` | done |
| 2 — group 0 gains ORM+height / detail / macro, neutral 1x1 defaults | done |
| 3 — `MaterialParams` packed into the 32-slot block, slot map pinned | done |
| 4a-4j — all twelve layers, each with WGSL + CPU reference + real-adapter parity | done |
| 4k — `SurfaceOut` gains `transmission`; the light loop accumulates it | done |
| 5 — composition into one `axiom_surface` | in flight |
| 6 — **selection**: how a material opts in | **open, see below** |
| 7 — app wiring, serve, screenshot | blocked on 5 and 6 |

`gpu-backend` is 438/438 including every GPU parity test on a real adapter, and
`cargo xtask check-architecture` passes.

## The open decision: how does a material select this shader?

The engine already has exactly one mechanism for "this material has a program":
an app authors an `axiom_surface::Surface`, the backend generates WGSL for it,
and the draw names the surface's **content digest**. The runtime material shader
is a program with no `Surface` behind it, so it has no digest and no way to be
named. Four candidates:

**A. `Surface` gains a kind.** `Surface { bindings, lighting, layers }` grows a
discriminant saying "this is the runtime material shader, and here are its
parameters". `cache::generate` returns the hand-written program instead of a
generated one. Everything else — content addressing, the preparation barrier,
one pipeline per distinct program, the cap — works unchanged, because the digest
covers the new field.

*Correct, and it is the design the rest of the system is already shaped for.*
Cost: it touches a **layer** crate broadly — the digest, the builder, `flatten`,
`surface_bytes`, `inspect` — all under the Branchless and Coverage Laws, and it
adds a concept to the authoring vocabulary **every app in this repo sees**.

**B. A reserved program id inside `gpu-backend`.** Contained, but circular: a
draw's `surface_program` is recovered from the material, and a material names a
program by *surface digest*. Without a `Surface` there is nothing for an app to
name. Rejected.

**C. Put the flag on the host material contract instead.** `axiom_host` owns the
material/frame contract, so a material could carry the parameters directly. This
exposes something real: **`MaterialParams` is authored data, and authored data
does not belong in a module.** The WGSL belongs in `gpu-backend`; the parameter
block belongs in a layer, exactly as `Surface` (layer) and its generated program
(module) already split. Worth doing regardless of which option wins.

**D. A fixed `Surface` recipe both sides construct.** Zero layer changes: the
backend recognises the digest of a specific surface the app also builds. It
works today — and it is a magic-value handshake between two crates, which is the
kind of coincidence this codebase deliberately does not rely on. Rejected on
those grounds, and recorded here so nobody rediscovers it as a shortcut.

**Recommendation: A, with C's observation folded in** — the parameter block moves
to a layer, and `Surface` gains the discriminant. It is the larger change and it
is the one that leaves the engine honest: a runtime material becomes *a kind of
surface you can author*, carried by the mechanism that already exists, rather
than a second parallel path bolted alongside it.

That is an engine-API change touching every app's vocabulary, so it wants an
explicit decision rather than being folded into a port.

---

# Decision taken: A, with C folded in

## What landed

**C — the parameter block moved to a layer.** `MaterialParams`, `UvMode` and the
sRGB decode now live in `crates/axiom-surface/src/material_params.rs` and are
public. `modules/axiom-gpu-backend/src/material_shader/params.rs` is a
re-export plus one genuinely module-shaped function, `param_bytes`, which turns
the packed `[[f32; 4]; 32]` into the uniform's byte run — authored values are a
layer concern, transport is the module's.

The reason, recorded because it generalises: **an app authors these values, and
authored data does not belong in a module.** Leaving them in the backend forces
one of two inverted dependencies — an app depending on a GPU backend in order to
describe a material, or the host's material contract naming a module's type.

**A — `Surface` gained a kind.** `axiom_surface::SurfaceKind` is `Field` (the
default, and almost every surface) or `RuntimeMaterial(MaterialParams)`.
`axiom_surface::runtime_material(params)` authors one.
`surface_program::cache::generate` now short-circuits on it and returns the
hand-written program instead of running the generator — written as an `Option`
chain, not a branch, because the Branchless Law applies here.

## The property that makes this cheap

`Surface::digest` is **structural**: it excludes parameter *values* so retuning
one cannot force a recompile. A runtime material obeys the same rule — the
canonical bytes carry `SurfaceKind::code()` and never the `MaterialParams`
behind it. So **every runtime material in a scene is one program and one
pipeline**, differing only in the bytes written to its parameter buffer.

That is exactly what the source does, where every extended material shares one
shader and differs by uniforms. It fell out of obeying the existing rule rather
than being designed in.

## The wire format moved, deliberately

The canonical bytes gained a two-byte kind code in the header and the schema
stamp went **1.0 -> 2.0** — a major bump, because a 1.0 reader would misparse a
2.0 buffer from the very next field.

`crates/axiom-surface/tests/surface_golden.rs` caught this, which is its job.
Four recorded values moved (`PLAIN_BYTES` and its digest, `LAYERED_LEN`/`_HASH`/
`_DIGEST`, and `FLATTENED_DIGEST`) and each is re-recorded **with the reason
next to it**, following the precedent that file already set. The distinction
that matters is written down there too: a digest that moves because the *format*
moved is the golden working; a digest that moves because the *surface* moved
would be a defect — and the round-trip assertion is what tells the two apart.

## Still open

`cache::generate` references `material_shader::compose::MATERIAL_SURFACE_WGSL`,
which the composition agent is writing. That is the last unresolved symbol; the
rest of the selection path compiles.
