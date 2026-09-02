# Atlas Ledger Audit — what the repo's own record says about itself

_Repo-wide audit derived from the `ax` ledger (`.axiom-atlas/ledger/raw/*.ndjson`)
rather than from reading source first: where agents actually spend their effort,
which files move together, which questions the repo could not answer, and which
of those signals point at a real structural defect._

**Date:** 2026-09-01
**Corpus:** 14,812 rows, 2026-08-22 → 2026-09-01, 1,801 sessions, 7 agent
identities. `refs` 6,813 · `change` 2,666 · `q` 2,293 · `read` 1,286 ·
`apply`/`edit`/`write` 617 · `friction` 20.
**Method:** DuckDB over the raw NDJSON to rank churn, co-change, reconnaissance
and zero-result searches; then two parallel subagent investigations (gpu-backend
Rust↔WGSL coupling; shmup `scene/wiring` shape) reading real source, with every
headline claim spot-verified by the lead against the files named.
**Deliverable:** this file. No engine code was changed by the audit.

---

## 1. Executive summary

The ledger's value is that it ranks by *observed effort* rather than by opinion,
so it points at things a source-first audit walks past. Three findings, in
descending order of leverage:

**(1) Axiom's laws govern the 10% of the repo where almost no work happens.**
80% of change events and 89% of reconnaissance land in `apps/`, which is
explicitly outside the Branchless Law, the Coverage Law and the architecture
checker. Ten touches, across six files, in `crates/` — in eleven days. `apps/` is
also now the largest body of Rust in the repo (251,927 lines vs 205,185 in
`modules/`, 102,472 in `crates/`). This is the finding with real leverage;
everything else is downstream of it.

**(2) Both hotspots the ledger ranked first have the same root shape: a correct
abstraction exists, was bypassed, and the duplicates it was meant to prevent are
now load-bearing and documented as such.** In `apps/axiom-shmup` a `Subsystem`
trait with ten implementors and a topologically-sorting `Registry` sits unused
while a second composition root was grown by hand; in `modules/axiom-gpu-backend`
a shader-splicing mechanism that already solves the copy problem for surface
programs was not applied to the uniform declarations, and a third copy of
`struct Lights` drifted.

**(3) That drift is a live bug on `main`,** found by following the #1 co-change
pair. `SDF_WGSL` reads `items[0]` at byte 96 of a uniform buffer `pack_lights`
writes `items[0]` at byte 208 of — a 112-byte skew, verified. Nothing in the
toolchain, the test suite or the gate can see it.

One finding that is **not** new and is credited as such: the `#[cfg(target_arch =
"wasm32")]` blind spot was already established by
[`retained-state-audit.md`](retained-state-audit.md) on 2026-08-11, which built a
working two-pass dylint runner for exactly this reason. What this audit adds is
that the second pass still is not wired into `scripts/dylint-gate.sh`, and that
the blind spot now has a concrete casualty (finding 3).

---

## 2. Method, and what the corpus can and cannot support

Rows are one NDJSON line per `ax` invocation: `ts`, `session`, `agent`, `cmd`,
`query`, `scope{path,lang}`, `hits`, `zero_result`, `top_paths[]`,
`bytes_changed`, `duration_us`, `ok`. Writes are *observed* — a detached child
asks git what moved — so the change rows see edits made by any route, including
`sed`, scripts and other agents in the same checkout.

Three limits worth stating before any number below is trusted:

* **Rows are keyed on path, so a renamed file fragments into several entities and
  its churn silently disappears.** The apparent top-churn file,
  `apps/shmup/src/scene/app.rs` (52 touches), is the *same file* as
  `apps/axiom-shmup/src/scene/app.rs`: created as
  `apps/claude-of-duty/src/scene/app.rs` (`f9c66d7f`), moved under `apps/shmup/`,
  deleted by `78403267`, restored by `94f8890b`. Its true churn is the sum of
  three ledger paths. Any future churn cut should resolve paths through
  `git log --follow` first.
* **34% of change rows are noise.** 917 touches are a Chromium profile directory
  (`apps/shmup/.bootprofile-profile/…`) a profiling tool wrote into the app and
  nobody gitignored; 311 are assets/captures; 102 are nested agent worktrees under
  `.claude/worktrees/` double-counted as parent-repo changes. Every cut in this
  document filters these.
* **Session granularity is uneven.** 2026-08-22 alone carries 1,782 sessions
  (the port sweep, `AXIOM_ATLAS_SESSION` unset) against a single session on most
  other days. Co-change is therefore computed over 15-minute time windows, not
  sessions.

---

## 3. Finding 1 — the gates and the work are pointed at different halves of the repo

Every touch and lookup, bucketed by which of Axiom's laws governs the file:

| class | change touches | files | read/query lookups |
|---|---:|---:|---:|
| **`apps/` — no gates** | **1,598 (80%)** | 735 | **38,308 (89%)** |
| `modules/` — branchless + 100% coverage | 188 (9%) | 31 | 2,538 (6%) |
| `tools/` + `scripts/` — no gates | 240 (12%) | 42 | 124 |
| **`crates/` — branchless + 100% coverage** | **10 (0.5%)** | 6 | 1,066 (2%) |

Rust lines by class: `apps/` 251,927 · `modules/` 205,185 · `crates/` 102,472.
Three apps carry 227k of the app total — `axiom-shmup` 135,060, `burnt-rubber`
56,624, `end-zone` 35,090.

`apps/axiom-shmup/app.toml` names five layers and four modules, and annotates
three of those modules "Test-only." Its shipped engine surface is essentially
`engine` + `windowing` plus some dimensioned scalars. What it carries instead:

```text
19,165  weapons/     15,815  ai/         10,046  materials/
18,050  world/       14,705  scene/       9,898  physics/
 9,333  fx/           8,482  audio/       8,460  ui/
 5,822  player/       4,401  sky/           602  render/
```

`modules/axiom-physics` (11,445 lines — rigid-body world, sphere/box/capsule/plane
narrow phase, sequential-impulse solver with friction, exact rotation-aware
raycast) is not in shmup's manifest at all, while shmup carries `rigidbody.rs`
(957), `ragdoll.rs` (1,305), `penetration.rs` (511) and `bvh.rs` (1,285).

**The obvious counter-hypothesis does not hold, and the real one is worse.**
Literal copy-paste between apps is *not* the problem: only 51 symbol names are
defined in three or more apps, nearly all generic (`CameraPose`, `CameraTuning`,
`smoothstep`). The three big apps consume near-disjoint module sets —

| app | modules named |
|---|---|
| `shmup` | engine, windowing, gpu-backend, agent |
| `burnt-rubber` | engine, agent, input, visibility, audio, windowing, debug-overlay |
| `end-zone` | agent, engine, physics, physical-animation, figure, input, windowing, debug-overlay |

— so each is independently rebuilding *concerns* the engine already owns, in
shapes that cannot be reconciled later. That is a harder problem than duplication,
because there is no single extraction that fixes it.

**Corollary, computed with transitive closure through feature modules:** 21 of 44
modules (34,197 lines) are unreachable from any app — led by `sim-core` (12,691),
`text` (6,036), `net-protocol` (3,073), `animation` (2,561), `planetgen` (2,215),
`netcode` (1,595). All are held to 100% coverage and the Branchless Law. (`scene`,
`render`, `resources`, `webgpu`, `render-pipeline`, `canvas2d-backend`,
`recording`, `animation-authoring` and `physics` *are* reachable via the `engine`,
`windowing` and `physical-animation` feature modules; a naive direct-reference
count reports 29 unreachable and is wrong.)

This restates, with measurements, the concern
[`vertical-slice-audit.md`](vertical-slice-audit.md) raised qualitatively as
"app-reimplementation" and "secretly rebuilding engine concepts." The ledger's
contribution is the ratio: it is not an occasional lapse, it is where effectively
all the work now goes.

---

## 4. Finding 2 — the shmup composition root: shotgun surgery from a *bypassed* abstraction

`apps/axiom-shmup/src/scene/` is 20 files, 14,705 lines. Four of its ten `wiring/`
files are sim/draw twins: `look`↔`sky_draw`, `ai`↔`soldier_draw`,
`fx_audio`↔`fx_draw`, `weapons`↔`weapon_look`.

**Why `ax refs AiSystem` returned three hits.** A unifying abstraction exists —
`pub trait Subsystem` at `apps/axiom-shmup/src/registry.rs:70`, with `init`/
`fixed_update`/`update`/`late_update`/`resize`/`render`/`dispose`, a topologically
sorting `Registry` (`registry.rs:137-147`), and **ten non-test implementors**. It
is routed around. `AiSystem`'s three references are its own definition, its own
`new`, and its own `impl Subsystem`; the running game never names it.
`PlayerSystem` is identical. Those `refs` searches came back empty *of consumers*
because the consumer that should exist was bypassed — which is precisely the
signal the ledger exists to record.

The cause is stated in the source, verbatim, in three places
(`scene/composition.rs:11-17`, `engine.rs:135-136`, `world/system.rs:777`):

> "`Engine` was unreachable … `player` depends on `world` and `render`, neither of
> which was a `Subsystem`, so `Registry::resolve` failed the moment it was
> registered. Three shut doors — that one, a `Ctx` nothing outside `engine` could
> build, and phase signatures that carried no input — and the port routed around
> all three by growing a second root. **Every hand-inlined duplicate this port has
> found since is downstream of that decision.**"

`composition.rs:52-70` can register 6 of 11 subsystems and cannot register the
rest: `WeaponSystem::phases()` returns `&[]` and `AiSystem::update` steps with
`None, None`, because the phase signatures cannot carry the camera, player and
physics seams those systems need.

### 4.1 Quantified duplication

**The per-frame ordering exists three times, and the copy that ships is the one
nothing tests.** `scene/app.rs:281-300` and `scene/draw.rs:61-80` are
byte-identical, doc comment included. `draw::frame` has **zero callers**;
`app::frame` is called only from `app.rs`'s own tests. The browser runs a third,
hand-inlined copy at `boot.rs:407-483`, carrying this comment at `:427-430`:

> "The same three steps `frame` runs. This loop **INLINES** them rather than
> calling it, so anything added to `frame` alone silently never runs in the
> browser — which is exactly how the viewmodel appeared wired and was not. Keep
> the two in step."

A duplicate that already shipped a visible bug, documented as load-bearing rather
than removed.

**The YXZ camera-quaternion composition exists four times** — `draw.rs:43-45`,
`draw.rs:103-105`, `fx_draw.rs:1026-1028`, `fx_audio.rs:618-620` — each with a
comment asserting it must match the others (`fx_draw.rs:1018`: "Identical to
`scene::app::write_camera`'s composition, and it has to be"). Both failure modes
are silent: a skewed billboard, a misplaced audio listener.

**The bake → upload → `Material` → node pipeline exists four times**, with four
private uploaders (`install.rs:48`, `soldier_draw.rs:305`, `weapon_look.rs:261-272`,
`fx_draw.rs:975`) feeding eight separate `Material::lit(...)` chains, each with its
own parallel descriptor struct (`KeyLook`, `WeaponKeyLook`, `ParticleClass`,
`DecalClass`).

**State is split across two roots.** Sim-side wiring hangs off `Game`; draw-side
wiring hangs off `Scene` (`app.rs:77-101`). Adding one capability therefore
touches `wiring/<new>.rs` + `game.rs` + `app.rs` + `draw.rs` + `boot.rs` +
`install.rs` — **six files, one capability.** `install.rs:9-13` names this exactly:
`scene/app.rs` "had grown into a composition root, a scene installer, a frame loop
and a browser entry point at once — and therefore had to be edited five times for
every capability added to the game." Four files were carved out of it; `app.rs` is
still 492 lines holding `Scene`, `build` and a `frame`. The split relieved the
symptom; the cause is untouched.

The port keeps its own running count. `level.rs:212`: "That is the **fifth**
hand-inlined duplicate this port has produced, and it is the reason the lights
were missing."

### 4.2 The `look.rs`↔`sky_draw.rs` 100% co-change is an *engine* defect

One `SkyDriver` (`look.rs:291-304`) published through two mutually exclusive engine
contracts: `look.rs` derives the *lighting* (`key_light:409`, `ambient:429`,
`clear_color:437`, `indirect_fill:460`, `depth_fog:596`), `sky_draw.rs` derives the
*display* (`FrameSky::gradient`/`with_body`/`with_clouds`). The coupling is by
direct field access and is circular (`sky_draw.rs:102` imports `look`'s
`dome_shoulder`/`scene_radiance`; `look.rs:1243` calls back into
`sky_draw::visible_sky`), and the two must agree *numerically* or the frame shows a
seam: `sky_draw.rs:109-119` pins `HORIZON_UP = 0.208` = sin(12°) purely because
that is the elevation `look.rs`'s `clear_color` is measured at — "A horizon stop
measured anywhere else would draw a seam along the horizon of every outdoor shot."
Nothing but a screenshot catches a mismatch.

`sky_draw.rs:262-264` diagnoses it and correctly declines to fix it from the app:
"The honest fix is an engine one … That is a new capability, not a line here."
`axiom_host::FrameSky` and `DirectionalLight`/`FrameAmbient`/`FrameDepthFog` are
two independent contracts describing one atmosphere with no reconciliation, and
`FrameSky` spends a single `body_color` lane read three ways (disc, halo, cloud
sunward face), forcing `sky_draw.rs:246-264` to choose between a correct sun disc
and correct clouds.

---

## 5. Finding 3 — a live uniform-layout bug, and the verification gap that hid it

The #1 co-change pair was `scene_renderer.rs` ↔ `scene_wgsl.rs`: 15 of 16–18
windows, 94%. **That coupling is not the defect** — they are two halves of one
pipeline definition, and halves of a definition should move together. Adding a
lighting term will always touch the packer and the shader. What the number was
actually pointing at is a *third* copy, in the file nobody thinks of as "the
shader."

### 5.1 The bug (verified)

`SDF_WGSL` (`scene_renderer.rs:517`) declares its own `struct Lights` and binds the
**same** buffer as the mesh pass — `build_sdf_pipeline` pairs the SDF layout with
`lights_layout`, group 1 binding 0 (`scene_renderer.rs:1723-1725`, `:2809`).

* Mesh pass (`scene_wgsl.rs:59-142`): after `camera` come seven more `vec4` lanes —
  `fill_sky`, `fill_ground`, `fill_gain`, `fill_indirect`, `fill_dir`,
  `fill_ao_strength`, `fill_sun_dir` — then `items: array<Light,16>`.
* `SDF_WGSL` (`scene_renderer.rs:522-546`): goes straight from `camera` to `items`.

`pack_lights` writes `items[0]` at byte **208** (`LIGHTS_UBO_BYTES = 208 +
MAX_LIGHTS*32`, `scene_renderer.rs:769`); `SDF_WGSL` reads `items[0]` at byte
**96**. A 112-byte skew — 3.5 `Light` structs. The SDF pass's `items[0].v` is
actually `fill_sky`, `items[0].col` is `fill_ground`, `items[1]` is
`fill_gain`/`fill_indirect`. `count` sits at offset 0 and is correct, so the pass
loops the right number of times over the wrong memory.

Introduced by `a99fa035` ("most of shmup"), which moved the header from 96 to 208
bytes and updated `pack_lights` and `scene_wgsl.rs` but not the third copy. The
comment at `scene_renderer.rs:536-538` states the invariant it is violating: "this
pass binds the SAME lights UBO as the mesh pass, so its `Lights` declaration must
stay layout-identical."

### 5.2 Why nothing caught it

| checker | catches a Rust↔WGSL layout mismatch? |
|---|---|
| Rust compiler | No — WGSL is an opaque `&str` |
| wgpu | No — `min_binding_size: None` in **46 files**; that flag is wgpu's one automatic size check |
| naga | **Not a dependency of `axiom-gpu-backend` at all** |
| coverage gate | No — both files are `#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]`, `offscreen` is off by default (`Cargo.toml:48`), and `coverage.sh:41` runs `--workspace` with no `--all-features` |
| `surface_program/parity_lighting.rs` | A genuinely good real-device pixel test — but `#[cfg(all(test, feature = "offscreen"))]`, so it never runs in `cargo test --workspace` or in the gate |

There is no `#[repr(C)]` anywhere in the crate (`ax q 'repr\(C\)'` → zero hits) and
no `bytemuck::Pod`. `Lights` is a byte count plus a positional `Vec<u8>` packer
writing 48 flat `f32`s (`pack_lights:3166-3223`); `ShadowU` is `[f32; 24]` written
by raw index (`:1962-1990`), where `shadow_uniform[20]` and `ShadowU.atlas.x` are
the same lane and nothing in the language, the build or the suite knows it. Field
names exist only in the WGSL.

The capability masks live in **four** places: `axiom_host::RenderCapability`,
`scene_wgsl.rs:149-157`, `scene_renderer.rs:549`, and
`surface_program/parity_lighting.rs:56-61` — which also restates
`LIGHTS_UBO_BYTES` and flags that it is doing so.

### 5.3 The `live_gpu_binding.rs` coupling is a different thing

86%, and it is signature width rather than byte layout. `SceneRenderer::new` takes
13 positional parameters (`:1176-1203`), `record` takes 16 (`:1837-1892`). Every
new frame capability appends one and forces an edit in all three callers
(`live_gpu_binding.rs:955`, `offscreen.rs:47`, `surface_program/bound_image.rs:563`).
Commits `dd2ec58e`, `ac4d9294`, `08ddb2fb` and `0cd9205a` each touch the three
files for no reason but a widened signature.

### 5.4 The right mechanisms already exist in-tree

* `wgsl_template::scene_shader` (`:419-446`) already composes shader text by pure
  concatenation from several constants — the splice mechanism finding 5.1 needs.
* `pack_lights:3211-3216` already reads `crate::indirect_lighting::FILL_DIR` /
  `AO_STRENGTH` from Rust rather than duplicating them as WGSL literals, with a
  comment at `scene_wgsl.rs:131-135` explaining why.
* `gbuffer.rs` is the model: declared unconditionally (`lib.rs:119`), compiled and
  covered natively, and carrying its own layout self-checks (`:889-891`).
  `lib.rs:190-193` states the principle: *"Gating a string on a rendering feature
  is the mistake `material_shader/compose.rs` already had to undo."*
* `gpu_backend_api/mod.rs:579-597` already publishes `bake_program_wgsl`
  specifically so validity can be "a naga parse of this string, costing no device
  and no feature flag."

Both patterns are already the right answer; they have not been applied to the
uniform declarations themselves.

---

## 6. The wasm32 blind spot — known since 2026-08-11, still not gated

**Credit where due:** [`retained-state-audit.md`](retained-state-audit.md) lines
3–8 established this and built the fix. `scripts/retained_state_audit.py` runs the
rulebook twice —

```sh
cargo dylint --all -- --all-targets                              # native
cargo dylint --all -- --target wasm32-unknown-unknown --lib      # wasm32
```

— "because a lint only sees what the compiler compiles: the platform-facing
modules' browser arms are `#[cfg(target_arch = "wasm32")]` and are entirely
invisible to the canonical native invocation." That second pass found **78 findings
no other pass could see.**

What this audit adds:

* `scripts/dylint-gate.sh:37` still runs **only** `cargo dylint --all --
  --all-targets`. The proven two-pass technique exists in the repo, for one lint,
  in a standalone script, and is not wired into the gate — so
  `engine_no_branching` and every other rulebook lint remain native-only.
* The scale: **100 `cfg(target_arch = "wasm32")` sites across 27 spine files** —
  34 in `modules/axiom-windowing/src/windowing_api/web.rs` alone, 12 in
  `gpu_backend_api/mod.rs`, 8 in `overlay_api.rs`.
* `scripts/coverage.sh` cannot close its half at all (no wasm profiler runtime),
  so the Coverage Law's stated scope — "drawn at the app/tooling edge and nowhere
  else" — is factually wider than the gate can enforce.
* It now has a casualty. Finding 3 lives in exactly this shadow.
* CI has been `workflow_dispatch`-only since 2026-07-14 (toolchain move to clippy
  1.96), so none of the gates run automatically at present.

An observed example, found by following the ledger's read-pressure ranking rather
than by looking for it: `modules/axiom-windowing/src/windowing_api/web.rs:2291`
contains a plain `if` inside a module bound by the Branchless Law. It is legal only
because the native dylint arm never compiles it.

---

## 7. What agents could not find

189 zero-result text searches (`q`/`file`), 632 zero-result `refs`. Two patterns
dominate, and both are tool-shaped rather than repo-shaped:

**Per-file sweep loops.** `#[test]` returned zero 13 times — each scoped to a
single file (`--path axiom-shmup/src/ai/agent.rs`, `…/animator.rs`, …). Agents run
one query per file because there is no way to get a per-file breakdown from one
query. 1,745 of 2,293 `q` calls are scoped, much of it census looping. An
`ax q --files-with-matches` / `--count-by-file` would collapse hundreds of
invocations into one.

**Searching a reference root.** `Math\.hypot`/`hypot` returned zero 20 times in one
session; the agent was searching `apps/shmup/*.js` source it could only *read*, not
*query*. Logged as friction **three separate times** (`31fd947e`, `0701be48`,
`0b39e6173`) in different words — the FNV normalisation did not collapse them, so
the repeat count is understated and the backlog looks three items long instead of
one item logged thrice. Verified still open: `ax q` has no `--ref` flag.

The 632 zero-result `refs` are almost entirely the 2026-08-22 port census
(`weapon_fire`, `bullet_impact`, `rim_uniform`, …) — the semantic index covers Rust
only, so a JS-side identifier can never resolve. Those are not repo illegibility
and should be excluded from any `ax miss` ranking that is meant to be read as a
work-list.

**Friction:** 20 rows, 10 still open at the time of writing. Nine are tool gaps.
One carries `verdict=repo` and is still live: on a device with no WebGL2 context
the app dies with `FATAL — no render backend available` instead of falling back to
Canvas2D, because the canvas context type cannot be reclaimed after a failed bind
(`modules/axiom-windowing/src/windowing_api/web.rs:2328`). That is exactly the
device class the fallback exists for.

_(One open row, `a9a1ef0b` — `cite` silently returning zero for a regex — was fixed
while this audit was being written; see the "One path-pattern language" section of
CLAUDE.md. It is a good illustration of the mechanism working: a zero that is a lie
was logged, triaged `tool`, and closed.)_

---

## 8. Ledger fidelity — fix these before trusting future cuts

* **337 paths are stored with git's C-style quoting intact** (`"apps/shmup/.bootprofile-profile/Default/Code Cache/js/…"`).
  `git status` quotes paths containing spaces; the observer does not unquote them,
  so one directory splits into a phantom second entity. Small fix in
  `tools/axiom-atlas/src/observe.rs`; it makes every future path aggregation
  correct.
* **`apps/shmup/.bootprofile-profile/` is not gitignored** — 917 change rows, 34%
  of the change log, are a Chromium profile directory.
* **Nested agent worktrees under `.claude/worktrees/` are observed as parent-repo
  changes** — 102 rows double-counted.
* **Churn should follow renames.** See §2.

---

## 9. Work order

Ordered by dependency, not by size. Items 1–3 are independently correct and can
land immediately.

**Immediate, no decision required**

1. **Add the seven missing `vec4` lanes to `SDF_WGSL`'s `Lights`**
   (`scene_renderer.rs:522-546`). Live bug; fix regardless of everything else.
2. **Delete `scene/draw.rs`'s dead duplicate `frame`** — zero callers, byte-identical
   to `app.rs:281-300`, and one of three copies of a path that already shipped a bug.
3. **Ledger fidelity:** unquote git paths in `observe.rs`; gitignore
   `.bootprofile-profile`.

**Structural, gpu-backend (Feature Module tier; no law or manifest change)**

4. **One declaration per buffer, spliced not copied.** Hoist `struct Light` /
   `struct Lights` / the group-1 binding / the `CAP_*` constants into a single
   `LIGHTS_UBO_WGSL` and splice it into both shaders through the concatenation
   `wgsl_template::scene_shader` already performs. Makes the class of bug in (1)
   structurally impossible. Nearly free — a text move plus one splice call; strings
   carry no coverage bill. **Highest leverage on this list.**
5. **Stop waiving wgpu's size check.** Replace `min_binding_size: None`
   (`scene_renderer.rs:2452`) with `NonZeroU64::new(LIGHTS_UBO_BYTES)`. Honest
   caveat: this catches *too small*, not *misaligned* — it would **not** have caught
   finding 3. Cheap insurance, not the fix.
6. **A naga layout test in the default suite.** `naga` is already in `Cargo.lock` via
   wgpu 25, so a dev-dependency adds no tree. Parse `scene_shader_source()` and
   `SDF_WGSL`; assert the module validates, `Lights`'s computed size equals
   `LIGHTS_UBO_BYTES`, `ShadowU`'s equals `SHADOW_LIGHT_UBO_BYTES`, the `vs`/
   `vs_skinned` `@location` inputs match `vertex_layout()` plus the instance attrs
   in count/index/type, and the global group/binding set matches the layout entries.
   **Real cost:** for this to be a *default* gate, `scene_wgsl.rs` (free — two
   strings) and the pure half of `scene_renderer.rs` (`LIGHTS_UBO_BYTES`,
   `vertex_layout`, the three packers, `debug_probe`'s table) must move out from
   behind the `cfg` and into an unconditional module, and the packers then owe 100%
   coverage. Same split that already happened at `29b2b2ce`; same shape `gbuffer.rs`
   already has. Do **not** un-gate the wgpu half.
7. **Collapse `record`'s 16-parameter signature** into a `SceneFrame` value built in
   `frame_packet_adapter.rs` — already the pure, unconditional, covered adapter.
   Legal (`axiom-host` is a layer, already named by this module) and branchless (a
   struct literal). Removes edit-site multiplication; does not remove the coupling,
   which is inherent.

**Structural, shmup (app tier)**

8. **Widen `Ctx`** so `Subsystem::update`/`fixed_update`/`late_update` can carry this
   frame's input, camera pose, physics handle and render seam. Named as the blocker
   in three places in the source. Until this lands, 5 of 11 subsystems structurally
   cannot be registered, and every duplicate below is load-bearing.
9. **One composition root** — `Game`'s fifteen named fields and hand-kept construction
   order (`game.rs:207-350`) collapse into `compose()` + `Registry::resolve()`; the
   prepare-order checkpointing becomes a test of the registry instead of a crutch for
   its absence.
10. **One `Scene::frame`**, called by both the native harness and `boot.rs`.
11. **`CameraPose::rotation_quat()`** — one YXZ composition replacing four.
12. **One app-tier surface kit** — a single `*Look` descriptor plus one uploader,
    replacing four bake→upload→`Material` pipelines and eight `Material::lit` chains.

**Engine capability (lowest correct layer)**

13. **One sky-state contract** the engine derives both the lit terms and the drawn
    dome from, reconciling `FrameSky` with `DirectionalLight`/`FrameAmbient`/
    `FrameDepthFog`, and giving the cloud sunward face its own lane rather than a
    third read of `body_color`. This deletes the `look.rs`↔`sky_draw.rs` 100%
    co-change pair outright.

**Gates**

14. **Wire the wasm32 dylint pass into `scripts/dylint-gate.sh`.** The technique is
    proven and already in-tree (`scripts/retained_state_audit.py`); it is one
    additional invocation. Without it the Branchless Law is native-only across 100
    spine sites.
15. **Fix the Canvas2D fallback** so it survives a failed WebGL2 bind
    (`web.rs:2328`) — the one open `verdict=repo` friction.
16. **Decide what `apps/` is** (finding 1). Either the gate scope line is drawn in the
    wrong place, or the engine's vocabulary does not reach far enough and apps are
    rebuilding it. The shmup/`axiom-physics` overlap and the disjoint module sets
    across the three big apps both argue the latter. This is a policy decision, not a
    refactor, and it should be made explicitly rather than by accretion.

---

## 10. How to reproduce these numbers

```sh
duckdb -c "
CREATE VIEW ledger AS SELECT * FROM read_json_auto('.axiom-atlas/ledger/raw/*.ndjson', union_by_name=true);
CREATE VIEW touch AS SELECT ts, day, session, cmd, unnest(top_paths) AS path, bytes_changed
  FROM ledger WHERE cmd IN ('change','apply','edit','write','record');
-- co-change by 15-minute work window
WITH w AS (SELECT DISTINCT time_bucket(INTERVAL 15 MINUTE, ts) win, path FROM touch)
SELECT a.path, b.path, count(*) wins FROM w a JOIN w b ON a.win=b.win AND a.path<b.path
GROUP BY 1,2 HAVING count(*)>=5 ORDER BY wins DESC;
"
```

`ax sql` prints the same preamble. Remember the three caveats in §2 — filter the
noise buckets, follow renames, and window rather than group by session.
