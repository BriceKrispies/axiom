# Render fan-out brief — read this first

You are porting part of `C:/dev/Claude-of-Duty/src/render/` (5,827 lines, the
18-pass frame graph) or a directly-blocking engine gap, into Axiom. Read
`10-convergence-plan.md` for the dependency order and `01-engine-gaps.md` for
the gaps.

**This is the SPINE.** `crates/*` and `modules/*`. The app-side brief
(`07-fanout-brief.md`) does not apply. Different laws.

## The laws

- **Branchless Law.** The *Rust* you write contains **zero** control flow: no
  `if`/`else`, no `match`, no `for`/`while`/`loop`, no `&&`/`||`, no `?`, no
  `if let`. Use `cond.then_some(a).unwrap_or(b)`, table indexing
  `[b, a][usize::from(cond)]`, iterator adapters, arithmetic. Recipes in
  `docs/unbranching.md`. **Tests are exempt** — never contort a test.
- **WGSL is exempt and this is not a loophole.** Shader text lives in a `&str`;
  the `engine_no_branching` dylint reads Rust HIR, so a loop in a string literal
  is data. A raymarch stays a raymarch. Write the WGSL as the source writes it.
- **Coverage Law.** 100% of your new Rust — every region, line, branch, function
  — covered in the same change. If something cannot be reached, that is a design
  signal: say so, do not add a shim to host a test.
- **No console output anywhere, tests included.** `println!`/`eprintln!`/`dbg!`
  are rejected by `cargo xtask check-architecture` even inside `#[cfg(test)]`.
  Put figures in assertion messages. This has already cost this port six
  violations; do not add a seventh.
- **Module Law.** A module may not depend on another module. A layer may import
  only what its `depends_on` declares.

## How correctness is proven

The instrument already exists and works: a **CPU reference that is the semantic
definition**, plus **CPU↔GPU parity on a real adapter** with measured
tolerances. Read `modules/axiom-gpu-backend/src/surface_program/parity.rs` for
the pattern, and `material_shader/*.rs` for twelve worked examples.

```
CARGO_TARGET_DIR=C:/Users/Brice/AppData/Local/Temp/claude/shmup-agent-targets/<slice> \
  cargo test -p axiom-gpu-backend --lib --features offscreen <your::module>
```

Never a bare `cargo test`/`cargo check` — you will contend with the
orchestrator's target directory.

**Derive every tolerance from a measurement. Never fit one to the miss you
happen to observe.** A tolerance more than 10x looser than the hardware needs is
itself a failure. Assert the measurement so the justification cannot rot.

## Transcribe from the GLSL text, never from your own Rust

The source is GLSL in JavaScript strings, so there is no oracle to call. That
makes the transcription the risk, and this port has measured the cost: in `sky/`
alone, **ten** real defects were found where the Rust and the "independent"
transcription meant to check it shared the same misreading, because one person
wrote both. Two files read as covered while being untested.

Write the WGSL from the GLSL text. Write the CPU reference from the GLSL text.
Where they disagree, work out from the algorithm which is right before changing
either. Hunt specifically for:

- a **division rewritten as a reciprocal-multiply** (five of the ten),
- a **re-associated multiply chain**, or a vector-by-scalar chain folded into one,
- any grouping tidied for readability.

**Float arithmetic is not associative. The source's grouping is the
specification.**

## Traps, by name

- **GLSL `sign` is not `Math.sign` is not `f32::signum`** — GLSL's returns `0.0`
  for any zero. You are transcribing GLSL.
- **`fract(x)` is `x - floor(x)`**, not Rust's `%`, and world coordinates go
  negative. The likeliest place for a CPU reference to diverge.
- **`mix`/`clamp`/`step`/`smoothstep`** have exact GLSL definitions; WGSL's
  builtins are permitted to factor differently, so write them out (the
  precedent `surface_program::emit` already sets).
- **Storage width is part of the algorithm** — found five times in this port,
  most sharply as an `f32` `rotateY(PI)` carrying a shear of `-8.74e-8` where
  `f64` gives `1.22e-16`.
- **Colours through `new THREE.Color(hex)`** use three's `SRGBToLinear`
  (`(c*0.9478672986 + 0.0521327014)^2.4`), **not** the GLSL `(c+0.055)/1.055`
  form. Algebraically equal, numerically different, and this port already
  shipped the wrong one once behind a comment claiming it was the right one.
  Evaluate in `f64` and narrow once — see `axiom_surface::srgb_to_linear`.
- **Dead computation in the source is still part of the source.** Port it with a
  comment.
- **An enum used as a table index is order-dependent**, including when it is
  serialised. Preserve the source's order.

## What already exists — build on it, do not re-derive it

- `axiom_surface::Surface` / `SurfaceKind` — a surface is either field-authored
  or the hand-written runtime material. `LightingModel` is a three-valued
  discriminant selected by multipliers, never a branch, so N models cost N
  programs rather than 3N.
- `scene_wgsl.rs` — prefix + a program-shaped hole + suffix, spliced by
  concatenation in `wgsl_template::scene_shader`. The lighting maths, 5x5 PCF
  shadow, hemisphere ambient and fog live in the suffix.
- `material_shader/` — twelve composed layers of `materials/shader.js`.
- `post_chain.rs` — bright-pass, separable blur, composite, colour grade,
  upscale. The plumbing for an HDR chain exists.
- `hdr_target.rs` — `RenderCapability::HdrTargets`, RGBA16F/RG16F/R32F/Depth32F.
- `mip_chain.rs`, `texture_sampling.rs`, `shadow_cull.rs`.

## Report

At most 10 lines: what you added, what you pinned and at what **measured**
tolerance, any divergence from the source and why, what you could not port, and
exactly what you need from the orchestrator or a sibling.

Detail goes in `docs/work-manifests/shmup-port/notes/<slice>.md`.

**Do not commit.** Do not edit `lib.rs`, `mod.rs`, `Cargo.toml`, `scene_wgsl.rs`
or another slice's files — report the lines you need instead.
