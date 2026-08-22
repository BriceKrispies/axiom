# The parallel port plan — read this first

You are continuing a port of a JavaScript browser FPS into an Axiom app. This
file is the handoff: what the plan is, what is done, what is left, and the traps
that have already cost real time.

**Read alongside this:** `02-port-recipe.md` (the per-slice procedure and the
trap list), `05-port-status.md` (per-commit status), and `00-manifest.md` /
`01-engine-gaps.md` for the engine side.

## The plan

**Port everything at once, in parallel, without compiling or running tests.
Then merge it all and test it once, together.**

Concretely:

1. **Fan out one subagent per remaining source file or coherent group.** They work
   simultaneously, each in its own area.
2. **Agents do not run `cargo build`, `cargo check`, `cargo test`, or any gate.**
   Builds are slow, they serialise on the same target directory, and they are the
   main thing that limits how many agents can run at once.
3. **Agents DO produce goldens.** The capture harness is a Node script that
   imports the original JavaScript — it does not need the Rust to compile. This is
   the one verification step that survives the no-build rule, and it is the step
   that has caught every real bug in this port. Do not skip it.
4. **Then one integration pass**: compile everything, fix what does not build, run
   the whole suite, and work through the failures.

### Why this shape

Parallel agents across distinct files have worked well here all session. The
failures have all come from shared state — the build directory, `lib.rs`,
`mod.rs`, and the git index. Removing the build from the agent loop removes the
biggest contention point and lets far more run at once.

### The cost, stated honestly

Nothing is verified until the integration pass, so errors accumulate silently and
arrive together. Expect the merge to be substantial work: type mismatches at
seams, duplicate helper definitions, and module-wiring conflicts, all at once and
all interacting. Budget real time for it, and treat the integration pass as a
first-class phase rather than a formality.

Two things make it survivable, and both are non-negotiable:

- **Every slice ships a golden JSON captured from the original.** Without it, a
  slice that compiles is indistinguishable from a slice that is correct — and
  this port has produced several of the latter.
- **Every slice ships its own Rust test file** that reads that golden, even
  though it will not be run until integration. Writing it later never happens.

## Rules for every porting agent

Give each agent these. They are the distilled result of ~25 slices.

1. **Work only in your assigned file(s).** Never `git add -A`. Commit with an
   explicit pathspec — a bare `git commit -m` takes whatever is in the index and
   has already put a broken build on this branch once.
2. **Do not build or test.** Write the code and the test; the integration pass
   compiles.
3. **Faithfulness over elegance.** Keep the source's constants, names and call
   order recognisable so the port can be diffed against the original. Comment WHY
   at any site Rust forces a divergence.
4. **Capture goldens from the original**, emitting a JSON file the test reads.
   Never hand-copy values into Rust — that is its own transcription step with its
   own error rate, and it has already produced a wrong golden here.
5. **Never write a test that asserts what you think the answer should be.** Every
   expected value comes from running the original. Three hand-written assertions
   in one slice claimed counts the original does not produce; the port was right
   and the tests were wrong.
6. **Port source defects faithfully** and pin them with a test naming the quirk.
   Ten have been found so far. Do not silently "fix" one.
7. **Preserve every `rng.fork()` and every literal seed, in order.** Draw order is
   part of the contract — an extra or missing draw shifts every later value.
8. **Cite the source** at the top of each module:
   `//! Ported from Claude-of-Duty \`src/<path>.js:<first>-<last>\`.`

## The traps that compile and are wrong

Each of these has cost hours. Check for them **by name** in every slice.

- **Storage width is part of the algorithm.** The source uses `Float32Array` in
  several places — the physics contact scratch, the local contact probes, the
  baked BVH bounds and triangle data. Every value is rounded to f32 on store and
  read back rounded. Porting those as `f64` changes results by ~1e-8 and the
  divergence only shows up once that data is used. **Grep the source file for
  `Float32Array` before you start.** This defeated a full debugging session.
- **`sign` is not `signum`.** GLSL/JS `sign(0)` is `0`; Rust `f64::signum(0.0)` is
  `1.0`. Hand-roll a three-valued sign. Hit twice, in unrelated subsystems.
- **Euler order is a convention, not a spelling.** The source uses `'YXZ'` in some
  places and `'XYZ'` in others, and `axiom_math::Quat::from_euler_xyz` composes in
  the opposite order to Three's `'XYZ'`. Four separate bugs traced to this,
  including a camera that banked on its own.
- **Matrix storage order.** A quaternion→matrix conversion written row-major where
  the source is column-major flips every off-diagonal sign. It compiles, and it
  silently corrupts an inertia tensor.
- **Float arithmetic is not associative.** Do not tidy or reorder an expression.
  Folding two sequential adds changes the last bits.
- **An enum used as a table index is order-dependent** even when the lookup looks
  like a search. Merging two enums with different variant orders silently
  reindexed every per-surface audio recipe.
- **`Math.hypot` is not `sqrt(x*x + y*y + z*z)`** — it scales by the largest
  magnitude first and rounds differently.
- **A matching count is not proof.** A differing vertex/triangle count is
  definitely a different algorithm; an equal count can still hide a different weld.
- **Your comparator can be the bug.** A triangle sort keyed on a 5 mm grid
  mispaired sub-5 mm features and reported the gap between neighbours as error —
  producing a phantom 71 mm "divergence". Before widening a tolerance, check the
  instrument.
- **Dead computation in the source is still part of the source.** Port it with a
  comment rather than dropping it.
- **`Math.round` is not `f64::round`, and `floor(x + 0.5)` is not `Math.round`
  either.** JS breaks ties toward `+Infinity`; Rust breaks them away from zero.
  They differ on **every negative half-integer**, and `Math.round(-0.5)` is
  `-0`, not `-1`. The obvious fix, `(x + 0.5).floor()`, is *also* wrong: for
  `x = 0.49999999999999994` adding `0.5` rounds to exactly `1.0`, so it returns
  `1` where JS returns `+0`. ECMA-262 states the two sub-0.5 clauses before it
  mentions flooring, precisely to head that off. Six slices independently
  rediscovered the tie rule and two of them shipped the naive form. It decides
  real structure, not presentation: in `physics/ragdoll.js` the rounded value
  decides whether two bone endpoints merge into one particle. Use
  `crate::jsmath::round`.
- **Do not write your own copy of a JavaScript builtin — `crate::jsmath` owns
  them.** `hypot2/3/4`, `sign`, `round`, `or_one`, each transcribed from V8 once
  and pinned bit-for-bit with no tolerance. Before this module existed the crate
  held six `hypot3`s across three different algorithms and nine three-valued
  `sign`s. Two `hypot3`s were wrong; `audio/spatial.rs` shipped the plain root
  with a comment reasoning the difference was "within a couple of ULP", and
  `ai/geo.rs` then shipped the same wrong form **citing that comment as its
  justification**. A duplicated primitive does not merely cost duplication — it
  lets two copies disagree and hands each one a plausible local excuse.
  (`Math.hypot`'s disagreement with the plain root was then measured
  independently by five slices: 25%, 36%, 37.5%, 38%, 41% of inputs. It is not
  marginal.)
- **`Vector3.length()`/`distanceTo()` are NOT `Math.hypot`.** They really are
  `sqrt(x*x + y*y + z*z)`. Converting them to `jsmath::hypot3` is the trap run
  backwards, and it is easy to do once you have been bitten the other way. Check
  which function the source actually calls at each site.
- **A transcription that nothing calls is worse than no transcription.** Both
  sky slices found `tests/sky/capture.mjs` holding complete hand-transcriptions
  of `CLOUDS_GLSL`, `SKY_BODY` and every volumetrics shader that were assigned
  to nothing and asserted on by no test. The harness read as finished; the
  coverage was zero. Before you trust an existing capture script, check that
  every function it defines is actually called and every key it writes is
  actually asserted.
- **Two transcriptions by the same author share the same misreadings.** Where
  the source is GLSL in a JS string there is no oracle, so the capture script
  must hand-transcribe it — but if you write that transcription by reading your
  own Rust, it will agree with your Rust and prove nothing. Ten real defects
  were found this way in `sky/` alone (divides written as reciprocal-multiplies,
  re-associated multiply chains, vector-by-scalar chains folded into one
  multiply), every one present identically on both sides. **Transcribe from the
  shader source text alone, before or without reading the Rust**, then diff.
- **Your harness's stubs are part of the comparator.** An `ai/agent` capture
  stub returned colliders with no `setSegment`, so the probe *fabricated seven
  colliders* onto an agent the real constructor gives none — and reported the
  port as wrong. A `weapons/viewmodel` capture stubbed materials as bare
  `MeshBasicMaterial`s, which defaulted an opacity to 1 and hid a genuine port
  error; both sides were wrong and it only surfaced because they were wrong
  *differently*. Instantiate the real object wherever you can.
- **`JSON.stringify(NaN)` is `null`,** and so are `Infinity`/`-Infinity`. A
  golden that pins a non-finite guard case silently round-trips it to null and
  the test panics on a type error rather than a value mismatch. Write non-finite
  values as tagged strings — or, better, write every float as its IEEE bit
  pattern in hex, which also carries `-0` (JSON cannot represent that either).

## What is done

Roughly two-thirds of ~65,000 source lines, all golden-verified against the
original. See `05-port-status.md` for the per-commit table. Highlights: the
deterministic core (RNG bit-exact), all of audio (47 voice graphs at zero ULP),
the weapon geometry kit and all 27 part builders plus three assemblies, the world
Assembler / ground / building kit / props / facade programme, all 19 procedural
surface generators, the physics BVH / capsule sweeps / character controller /
penetration / rigid bodies, the player movement and camera stack, the HUD, and
atmospheric scattering.

Engine-side additions: HDR render targets as a declared capability, capsule and
triangle geometry with swept tests in `axiom-math`, windowing carrying authored
surfaces, capsule contacts with real hit records, and `App::install`.

There is a walkable street rendering in the browser — buildings, collision,
ground, sky from real scattering — at under 150 draw calls.

## HAZARD: half-finished ports are on `main` and look finished

Four agents were stopped mid-slice and their partial work was committed. These
compile and are wired in, but are **incomplete and have no golden tests**:

| file | source lines | ported lines | |
|---|---|---|---|
| `ai/parts.js` | 1073 | **0** | **absent — see below** |
| `ai/geo.js` | 754 | **0** | **absent — see below** |
| `weapons/hands.js` | 1163 | 374 | ~32% |
| `sky/volumetrics.js` | 527 | 246 | ~47% |
| `weapons/viewmodel.js` | 1088 | 604 | ~56% |
| `ai/agent.js` | 1009 | 745 | ~74% |

**Correction (2026-08-21).** The first two rows were wrong. `apps/shmup/src/ai/parts.rs`
and `apps/shmup/src/ai/geo.rs` do **not** exist on `main` and never landed — the
partial work those line counts describe died with the stopped agents. `src/ai/`
holds exactly `agent.rs`, `grounding.rs`, `nav.rs`, `squad.rs` and `mod.rs`. Both
files are therefore a from-scratch port, not a completion. The correction matters
in the other direction too: it means the hazard is *smaller* than stated for those
two (absent code misleads nobody) and *larger* than stated elsewhere — `ai/nav.rs`
(861 lines) and `ai/squad.rs` (227) are fully ported, wired in, and have **no test
of any kind**; there is no `ai_*_port.rs` anywhere. An unpinned port is
indistinguishable from a wrong one, so they belong on this list.

Also check `sky/dome.rs`, `sky/clouds.rs` and `ai/grounding.rs`, which are
borderline.

**Deal with these first.** Either finish them properly with goldens, or delete
them and re-port from scratch. Right now nothing signals that they are unfinished,
which is worse than their being absent.

## What is left — roughly 21,000 lines

Assign one agent per row, or per file for the larger rows.

| area | lines | notes |
|---|---|---|
| `render/` (all) | 5,827 | The 18-pass frame graph: CSM, GTAO, TAA, SSR, bloom, EV100 metering, AgX, the material patcher. **Engine-side**, in `crates/axiom-host` and `modules/axiom-gpu-backend`, under the Branchless and Coverage laws — not app code, and not subject to the no-build rule in the same way. See `00-manifest.md` for the pass list and `01-engine-gaps.md` for the dependency order. |
| the subsystem facades | ~6,000 | Every `index.js`: physics (1059), fx (1316), ai (1107), sky (872), player (752), ui (613), world (445), weapons (843), materials (353). This is the integration layer — what turns ported subsystems into a running game. |
| `ai/` remainder | ~2,900 | `soldier` (837), `textures` (951), `animator` (559), `rig` (265), `weapon` (291). |
| `world/dressing.js` | 2,269 | Market stalls, wrecks, palms, laundry, rubble; scatter with exponential wall falloff and a camera-clearance guard. The pass that makes the street a place. |
| `physics/` remainder | ~2,160 | `ragdoll` (763, PBD), `index` (1059), `debug` (342). |
| `materials/shader.js` | 890 | The runtime material shader — POM, triplanar, de-tiling, detail/macro layers, weathering, curvature wear. **Belongs in hand-written WGSL in `gpu-backend`**, not in the field algebra (no loops, no derivatives, no sampling, 256-node budget). Nothing samples the 19 ported generators without this. |
| `ui/minimap.js` | 603 | Needs an orthographic depth bake read back once, then a Sobel pass. Blocked on the render work. |
| `player/lowhealth.js` | 172 | |
| `sky/fullscreen.js` | 101 | |

**Not work:** `core/prewarm.js` (solved structurally — the engine compiles surface
programs at a preparation barrier) and the `preview` / `selftest` / `demo`
harnesses (~2,300 lines of dev tooling).

## Suggested order

1. **Resolve the half-finished six.** They are a correctness hazard.
2. **The runtime material shader.** Unlocks 19 generators already ported and
   verified — the single biggest visual return per line.
3. **The facades.** Turns subsystems into a game.
4. **The render frame graph.** Largest, hardest, and the rest of the visual gap.

## The integration pass

When the fan-out completes:

1. `cargo check -p axiom-shmup` and fix until it builds. Expect duplicate helpers,
   seam type mismatches, and `mod.rs` / `lib.rs` conflicts.
2. `cargo test -p axiom-shmup`. Every slice's golden test runs for the first time.
3. Triage failures **by slice**, and for each one ask whether the port or the
   test is wrong — both have happened here.
4. `cargo xtask check-architecture` must pass.
5. Serve and screenshot. A green build and a painted page are different facts.

## Environment gotchas

- **The coverage gate cannot pass in this repo**, for three stacked reasons: the
  default gnu toolchain has no `profiler_builtins`; MSVC full-workspace linking
  OOMs (`link.exe 0xc0000142`); and two app test suites abort the run before it
  measures — one of them broken on `main` and untouched by this work. Use
  `RUSTUP_TOOLCHAIN=nightly-x86_64-pc-windows-msvc CARGO_BUILD_JOBS=2`. Details in
  `05-port-status.md`.
- Long **backgrounded** commands get killed unpredictably; foreground runs survive
  but the tool ceiling is 600 s.
- The shell cwd resets between calls — `cd` explicitly in every command.
- Piping a gate to `tail` discards its exit status. Redirect to a file.
- Never run two gates at once; dylint reports a phantom `cargo metadata` error and
  masks the real finding.
- Serve with `uv run scripts/localhost_servers.py start-app shmup --port 8080`,
  then drive `scripts/playwright_controller.py` and **read the screenshot**.

## Source of truth

`C:/dev/Claude-of-Duty` is read-only. Never modify it. It is ISC licensed and the
Three.js algorithms under the geometry kit are MIT — the `Ported from` citations
are required attribution as well as provenance, so keep them.

**When the port and a golden disagree, work out what the value should be from the
algorithm before changing either side.** And when something diverges and the
arithmetic all looks identical, go and read the original's variable declarations
— that is where the last one was hiding.
