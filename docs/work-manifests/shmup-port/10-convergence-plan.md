# Converging on the original — what is left, and how to drive it

This is the handoff for the last phase of the Claude-of-Duty port: getting
Axiom's render of the street to match the original's. Read
`06-parallel-port-plan.md` for the fan-out method and `08-material-shader-plan.md`
for the material shader that just landed.

## First, the honest bit: "pixel identical" is not the target, because it cannot be

Byte-equal framebuffers against the original are **not achievable**, and chasing
them would waste the effort. Four independent reasons, none of them fixable:

1. **Different renderer.** The source is Three.js on WebGL2; Axiom is wgpu on
   WebGPU. Rasterisation fill rules, depth precision, filtering and mip selection
   differ by specification, not by defect.
2. **Different shader compilers on different silicon.** This repo's own
   `surface_program/parity*` suite compares CPU against GPU with *measured
   tolerances* precisely because bit-equality is not available even between two
   implementations of the same expression. One material layer here is bit-exact;
   the rest land at one f32 ULP, and that is the good case.
3. **The GPU libm is unspecified.** The same `pow`/`sin` on two vendors differ in
   the last bits, which a fractional power amplifies — this port already measured
   that effect at 3.3e-5 relative on the *CPU* side (see the gnu-toolchain note in
   `06-parallel-port-plan.md`).
4. **The reference itself is not deterministic across machines.** The capture
   below runs under SwiftShader on this box and a real GPU elsewhere.

So the target is **measured visual convergence against a captured reference**,
scored on a rubric, with each change moving a number. That is exactly what the
`visual-convergence` skill in this repo does, and it is the right instrument.
Structural completeness — every source file ported and golden-verified — is the
other half, and it *is* binary.

## Prerequisite 0: a working reference capture

**Nothing downstream is measurable without this, and it does not currently work.**

The original ships its own harness:

```sh
cd C:/dev/Claude-of-Duty
node tools/capture.mjs --list                       # named shots
node tools/capture.mjs --shot=hero --out=shots/hero.png --w=1280 --h=720
```

Shots defined in `src/dev/shots.js`: `hero`, `interior`, `detail`, `sunset`, and
more. Each fixes `pos`, `look`, `fov` and `time`, which is what makes a
comparison meaningful at all.

**Current state: it times out.** `page.waitForFunction: Timeout 90000ms
exceeded` waiting on `window.__READY__`, running under
`ANGLE (SwiftShader)` — a software rasteriser. The material bakes alone log
~1.3 s and the physics BVH 57 ms, so the boot is slow but plausibly finishable;
either the timeout needs raising, a real GPU needs requesting, or something is
genuinely failing after the logs stop. **Diagnose this first.** A convergence
loop with no oracle is a loop that measures nothing.

Then capture the same camera from Axiom. `apps/shmup` must grow a matching
fixed-shot mode — same `pos`/`look`/`fov`/`time` — or the two images differ for
reasons that have nothing to do with the port.

## What is left, in dependency order

Ported so far: **~97,500 lines of Rust across 202 modules**, all golden-verified.

### 1. Bake and upload the nineteen generators (app tier)

The single biggest visual return per unit of work, and everything upstream of it
is already done and pinned. `materials/bake.rs` produces real
albedo/roughness/metalness/normal data; nothing uploads it. Until it does, POM,
de-tiling and the detail layer all sample the neutral 1x1s and contribute
nothing — three of the twelve composed layers are inert.

The original logs exactly what to produce: `bake gravel 1024px`, `asphalt`,
`concrete`, `dirt`, `metal_painted`, `plaster`, `wood`, `metal_rust`, `brick`,
`fabric 512px`, `corrugated`, `rubber`, `metal_brushed`, `foliage 512px`.

### 2. The lanes the composition asked for and does not have (engine)

Each is small; together they switch on layers that are currently wired but inert.

- `SurfaceIn.view_distance` — both distance fades are pinned at 1.0 today.
- `SurfaceIn.front_facing` — `owFaceDir` is hardcoded `+1`, so a back face reads
  its own normal rather than the flipped one.
- A real per-vertex **mask** lane. `vertex_color` now exists but is the vertex
  colour times the *instance* colour, which the material multiplies into albedo.
  The source's `vColor` is a wear/grime/AO mask — a different quantity that
  happens to share a name. Conflating them would tint by a mask and mask by a
  tint.
- `SurfaceOut.ao` — `aoStrength` has nowhere to go, so five layers' occlusion
  reaches only the cloth term.

### 3. The triplanar permutation (engine)

Nine fetches plus its own detail arm. A genuine second program shape, like
de-tiling — see `SurfaceKind`.

### 4. The render frame graph — 5,827 lines, wholly unported (engine)

**This is where the remaining visual gap actually lives.** No material work will
close it.

| file | lines | |
|---|---|---|
| `render/index.js` | 1696 | the frame graph itself |
| `csm.js` | 582 | 4x2048 cascades (the original logs `4x2048 CSM`) |
| `composite.js` | 353 | |
| `gtao.js` | 324 | |
| `materialpatch.js` | 321 | |
| `probe.js` | 306 | |
| `taa.js` | 272 | |
| `dof.js` | 241 | |
| `prepass.js` | 236 | |
| `exposure.js` | 218 | EV100 metering |
| `bloom.js` | 215 | |
| `ssr.js` | 197 | |
| `glsl.js`, `lut.js`, `contact.js`, `motionblur.js`, `env.js`, `pass.js` | 916 | |

The original boots with `ultra · 4x2048 CSM · taa:true gtao:true ssr:true
mb:true`. Axiom has one colour attachment and one depth buffer.

**Blocked on two engine gaps first** (`01-engine-gaps.md`):
- **G8** — no MRT, no G-buffer, no depth prepass, no velocity buffer. GTAO, SSR,
  TAA, motion blur and decals all need them.
- **G2** — no PBR BRDF. Blinn-Phong with a global `SPECULAR_POWER = 48.0`, no
  Fresnel, no GGX/Smith, no energy conservation. Needs a fourth `LightingModel`
  landed with the BRDF in both backends.

Also **G9**: 16 forward lights, one shadow-casting directional, one cascade. An
FPS with muzzle flashes will exhaust that.

### 5. The remaining subsystem facades — ~5,600 lines (app tier)

`ai/index.js` (1107), `physics/index.js` (1059), `weapons/index.js` (843),
`sky/index.js` (872), `world/index.js` (445), and finishing `fx/index.js` (1316,
partial). These turn ported subsystems into a running game rather than a static
street. Done already: `audio`, `materials`, `ui`, `player`.

### 6. `ui/minimap.js` (603) — blocked on the render work

Needs an orthographic depth bake read back once, then a Sobel pass.

## How to drive this with a loop

Two different loops, for the two different halves. Do not mix them.

### A. The structural port — fan-out, integrate, verify

Use `/loop` with a prompt naming this file. Self-paced (omit the interval) so
each iteration runs to completion rather than firing on a clock:

```
/loop Read docs/work-manifests/shmup-port/10-convergence-plan.md. Take the next
unstarted item in dependency order, fan out one subagent per file or coherent
group per 06-parallel-port-plan.md and 07-fanout-brief.md, then integrate: wire
the modules, run cargo test for every crate you touched, run cargo xtask
check-architecture, and update this file's status. Do not start an item whose
dependencies are unmet. Report what landed and what it cost.
```

Non-negotiables that the briefs already encode, and that make the loop safe:
every slice ships a golden captured by running the original; agents do not build
or commit; the orchestrator owns `mod.rs`, `lib.rs` and the git index.

### B. The visual convergence — champion/candidate against the reference

Once prerequisite 0 works, use the repo's own instrument:

```
/visual-convergence <path-to-reference.png>
```

It runs a disciplined champion/candidate loop: capture the real app, score
against the reference on a rubric, make one bounded change, re-score, keep it
only if the number moved. `/visual-convergence-propose` fans that out across
seven lenses (art direction, lighting, colour, surfacing, modelling, rigging,
engine feasibility) in isolated worktrees and lets you cherry-pick.

**Do not start B before the render frame graph exists.** Scoring a Blinn-Phong,
no-GTAO, no-TAA, LDR image against an AgX-tonemapped reference will report a
large constant gap that no bounded nudge can close, and the loop will thrash on
cosmetics while the structural cause sits untouched. Item 4 is the work; B is how
you finish it afterwards.

## Where things stand right now

Green: `axiom-surface` 117, `axiom-gpu-backend` 451 (incl. real-adapter GPU
parity), `axiom` 154, `axiom-shmup` 1271. `cargo xtask check-architecture`
passes. Uncommitted.

Note the workspace also has **pre-existing** failures in `apps/burnt-rubber`,
`apps/end-zone` and `apps/axiom-zanzoban` — 13 targets — confirmed present at a
clean HEAD checkout and unrelated to this port. Do not let a loop chase them.
