# Final wave brief — write everything, build nothing

This wave ports the last ~5,000 source lines at once. It **overrides** the parts
of `11-render-fanout-brief.md` (spine) and `07-fanout-brief.md` (app) that it
names, and nothing else. Read your tier's brief too.

## The overrides

1. **Do not build. Do not test.** No `cargo build`, `check`, `test`, `clippy`,
   `fmt`, `xtask`, no gate, no `--features offscreen` parity run. Builds
   serialise on one target directory and are the thing that limits how many of
   you run at once. The orchestrator compiles and runs everything in one
   integration pass afterwards.

   **This means your GPU parity test will not run before you finish.** Write it
   anyway — see below.

2. **Do not commit. Do not stage. No mutating git command** (`add`, `commit`,
   `reset`, `checkout`, `stash`, `clean`, `pull`, `merge`, `rebase`). Read-only
   git is fine. The orchestrator commits.

3. **Do not touch `mod.rs`, `lib.rs`, `Cargo.toml`, `app.toml`,
   `scene_wgsl.rs`, or another slice's files.** Every one is shared and this
   wave is wide. End your report with the exact lines to add, e.g.
   `modules/axiom-gpu-backend/src/lib.rs: mod gtao;`.

4. **Write only your assigned paths**: your module file(s), your test file, your
   golden directory, and `docs/work-manifests/shmup-port/notes/<slice>.md`.

## What still binds, and matters more than usual

Because nothing compiles until the end, the discipline is the only thing
standing between you and a slice that looks finished and is wrong.

- **Ship the test even though it cannot run.** Writing it later never happens,
  and the integration pass needs it the moment the crate builds. State the
  tolerance you *expect* and say it is unverified — the orchestrator will run it
  and hand you the real number.
- **Transcribe from the GLSL/JS source text, never from your own Rust.** This
  port has measured the cost: ten defects in `sky/` alone where the Rust and the
  "independent" transcription meant to check it shared one misreading. Hunt for
  a division rewritten as a reciprocal-multiply (five of the ten), a
  re-associated multiply chain, any grouping tidied for readability.
- **Float arithmetic is not associative. The source's grouping is the
  specification.**
- **Preserve every `rng.fork()` and literal seed, in order.**
- **Port source defects faithfully** and pin them by name.
- **Storage width is part of the algorithm.** Grep for `Float32Array` /
  `HalfFloatType` before you start.
- **A deferral needs an expiry check.** Four defects this port has found were a
  justified "not yet ported" that silently became a defect when its blocker
  cleared — including a 765-line file (`interiors.rs`) that was never compiled,
  and a bullet path that could never hit anything. If you defer something, say
  what would make it live and name the file that must change.

## The traps, by name

GLSL `sign` is not `Math.sign` is not `f32::signum` (GLSL's returns `0.0` for any
zero) · `fract(x)` is `x - floor(x)`, not `%`, and world coordinates go negative
· `Math.hypot` is V8's max-scaled Kahan sum (`crate::jsmath` in the app), but
`Vector3.length()` genuinely is the plain root — check which the source calls ·
`Math.round` ties toward `+Infinity` · `|0` is `ToInt32` and **wraps** where Rust
`as i32` saturates · `MathUtils.lerp` is `(1-t)x + ty`, not `a+(b-a)t` ·
`MathUtils.smoothstep(x, min, max)` has GLSL's arguments reversed · a hex colour
through `new THREE.Color()` uses three's `SRGBToLinear`, evaluated in `f64` and
narrowed once (`axiom_surface::srgb_to_linear`) · GLSL `mat3(a,b,c)` takes
**columns** · an enum used as a table index is order-dependent · dead computation
in the source is still part of the source · `Time::default()` derives
`scale: 0.0`, so `elapsed` never advances.

## Spine slices: the laws still bind

Branchless Rust outside tests (the WGSL inside a `&str` is data — a loop there is
fine), 100% coverage of new Rust, and **no `println!`/`eprintln!`/`dbg!`
anywhere, tests included** — `cargo xtask check-architecture` rejects them even
in `#[cfg(test)]`, and this port has already had to remove six. Put figures in
assertion messages.

## What exists — build on it

- `gbuffer.rs` — oct view-normal + coverage, velocity (multiply `y` by
  `VELOCITY_TEXTURE_V_SIGN`), linear view depth, prepass depth. Bind via
  `GBufferTargets::view(GBufferChannel::{Normal,Velocity,Depth})`, decode with
  `gbuffer::decode_normal` (**view space**).
- `cascade/` — 4×2048 CSM. `bloom_pyramid/` — the 6-level pyramid.
  `exposure.rs` — EV100 metering. `agx.rs` — the AgX curve.
- `material_shader/` — twelve composed layers of `materials/shader.js`.
- `test_gpu.rs` — **the one shared GPU fixture**. Do not create a
  `wgpu::Instance`; 20 sites doing that crashed the driver.
- `axiom_surface::{Surface, SurfaceKind, MaterialParams, runtime_material}`.

## Report

At most 10 lines: what you wrote, the WGSL/Rust entry points and their
signatures, the tolerance you expect (flagged unverified), the wiring lines the
orchestrator must add, anything you could not port, and anything you need from a
sibling.
