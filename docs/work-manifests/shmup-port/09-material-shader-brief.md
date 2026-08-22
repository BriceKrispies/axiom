# Material-shader fan-out brief — read this first

You are porting one layer of `C:/dev/Claude-of-Duty/src/materials/shader.js` into
`modules/axiom-gpu-backend/src/material_shader/`. Read
`08-material-shader-plan.md` for the whole shape; this file is the contract.

**This is the SPINE, not an app.** The app-side brief (`07-fanout-brief.md`) does
not apply. Different laws, different rules. Read this one.

## Your file

You own exactly one stub, `material_shader/<layer>.rs`, plus its tests. It holds
three things that land together:

1. **The WGSL**, as a `pub(crate) const <LAYER>_WGSL: &str`.
2. **A CPU reference** in Rust computing the same maths.
3. **A parity test** proving the two agree on a real adapter.

Do **not** touch `material_shader/mod.rs`, `lib.rs`, `scene_wgsl.rs`,
`surface_program/`, `Cargo.toml`, or another layer's file. They are shared, and
every layer is being written at once. If you need something from a sibling
layer, say so in your report — do not reach into it.

## The laws that bind you

- **Branchless Law.** The *Rust* in your file contains **zero** control flow: no
  `if`/`else`, no `match`, no `for`/`while`/`loop`, no `&&`/`||`, no `?`, no
  `if let`. Use `cond.then_some(a).unwrap_or(b)`, table indexing
  `[b, a][usize::from(cond)]`, iterator adapters, and arithmetic. The recipe
  catalogue is `docs/unbranching.md`. **Tests are exempt** — never contort a test.
- **The WGSL is exempt, and this is not a loophole.** Shader text lives inside a
  `&str`; the `engine_no_branching` dylint reads Rust HIR and a loop in a string
  literal is data, not control flow. Parallax occlusion mapping *is* a loop and
  must stay one. Write the WGSL as the source writes it.
- **Coverage Law.** 100% of your Rust — every region, line, branch, function —
  covered by tests in the same change. New code ships with its coverage or it is
  not done. Do not add a shim, a pass-through, or a dead arm just to be covered;
  if something cannot be reached, that is a design signal, so say so.
- **Module Law.** `gpu-backend` is a module. Do not add a dependency on another
  module.

## How correctness is proven here

This repo already has the right instrument, and it works: a **CPU reference that
is the semantic definition**, plus **CPU↔GPU parity on a real adapter** with
measured tolerances. Read `surface_program/parity.rs` before you write anything —
it is the pattern, including how it acquires an adapter and fails loudly rather
than skipping.

Run it with the feature on, and with an isolated target directory so you do not
contend with the orchestrator's:

```
CARGO_TARGET_DIR=C:/Users/Brice/AppData/Local/Temp/claude/shmup-agent-targets/<layer> \
  cargo test -p axiom-gpu-backend --lib --features offscreen material_shader::<layer>
```

**A tolerance more than 10x looser than the hardware needs is itself a failure.**
Derive it from a measurement, never fit it to the miss you happen to observe.

## Transcribe from the GLSL text, not from your own Rust

The source is GLSL held in JavaScript strings, so there is **no oracle to call**.
That makes the transcription itself the risk, and this port has already measured
the cost: in `sky/` alone, **ten** real defects were found where the Rust and the
"independent" JS transcription meant to check it contained the *same* misreading,
because one person wrote both by reading the other. Two whole shader files read
as covered while being untested.

So: write the WGSL from `shader.js`'s GLSL text. Write the CPU reference from the
same GLSL text. Where they disagree, work out from the algorithm which is right
before changing either. Look specifically for:

- a **division turned into a reciprocal-multiply** (five of the ten were this),
- a **re-associated multiply chain**, or a vector-by-scalar chain folded into one
  multiply,
- any grouping tidied for readability.

**Float arithmetic is not associative. The source's grouping is the
specification.** Transcribe it literally, however clumsy.

## Traps, by name

- **GLSL `sign` is not `Math.sign` is not `f32::signum`.** GLSL's returns `0.0`
  for any zero; `signum` returns `±1.0`. You are transcribing GLSL — match GLSL.
- **`mix`/`clamp`/`step`/`smoothstep`/`fract`** have exact GLSL definitions.
  `fract(x)` is `x - floor(x)`, which is *not* Rust's `%`. WGSL matches GLSL for
  these; the CPU reference is where they get written wrong.
- **`Math.hypot` is not `sqrt(x*x+y*y+z*z)`**, and `Vector3.length()` *is*. Check
  which the source calls.
- **Storage width is part of the algorithm.** Found five times in this port, most
  sharply as an `f32` `rotateY(PI)` carrying a shear of `-8.74e-8` where `f64`
  gives `1.22e-16`. GPU is `f32`; if your CPU reference computes in `f64` and the
  GPU in `f32`, your tolerance must account for it — say which you chose and why.
- **Dead computation in the source is still part of the source.** Port it with a
  comment rather than dropping it.
- **A `#define` is not a function.** `OW_CLOTH_LIGHT` and friends are textual
  macros expanded per light; expanding them by hand changes evaluation order if
  you are careless.

## The calling convention

Write each layer as a **free function taking explicit arguments** — including
textures and samplers, which WGSL permits as function parameters. Do not read
globals, do not reach for `params.slots` directly, do not assume a binding index.
The orchestrator composes the layers into `axiom_surface` and wires the bindings;
that is what lets twelve layers be written at once without sharing a file.

`SurfaceIn` now carries world space (`world_pos`, `world_normal`, `view_dir`)
alongside the object-space lanes — added for exactly this shader, and proven not
to move any existing generated program.

## Report

At most 10 lines: the WGSL entry points you defined and their signatures, what
you pinned and at what measured tolerance, any divergence from the source and
why, anything you could not port, and anything you need from a sibling layer.
Detail goes in `docs/work-manifests/shmup-port/notes/material-<layer>.md`.

**Do not commit.** Do not run a bare `cargo test`/`cargo check` without
`CARGO_TARGET_DIR` set.
