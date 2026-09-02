# Waves, assignments, and the partitioning rule

## The partitioning rule

> **One writer per file. Assign by file, never by concept. A file that more than
> one slice would need is either split in W0 or is read-only for the whole wave.**

**Never written by any W2a agent:** `src/lib.rs`, every `mod.rs` (23 of them),
`Cargo.toml`, `app.toml`, `app.json`, `.gitattributes`, `src/characterize/**`,
`tests/golden/**`, anything under `src/materials/wgsl/` (another session's live
WIP).

**Read-only shared dependencies** (read freely, never edit): `src/rng.rs`,
`src/jsmath.rs`, `src/world/palette.rs`, `src/world/kit/mod.rs`, `src/world/geo.rs`,
`src/weapons/geometry/**`, `src/fx/particles.rs`, `src/fx/util.rs`,
`src/physics/surfaces.rs`.

**Tests live in-file.** No agent writes into `apps/axiom-shmup/tests/`. That
deletes an entire collision surface the port had. See `01-agent-brief.md` §6.

**`Surface::ALL` ordering is frozen** (`src/world/palette.rs:32`). It is the index
basis for `foley::IMPACT`, `foley::STEP`, and the physics per-triangle surface
byte. Nobody reorders it, and any new table indexed by it says so in a doc
comment.

### For the engine track (W2b)

Two agents adding an op both need `field_op.rs` and `dispatch.rs`. So:

> **The orchestrator assigns the discriminants in the wave brief. One "vocabulary
> agent" per crate owns exactly `field_op.rs` + `dispatch.rs` + `lib.rs`
> re-exports. Every other agent owns exactly one `ops/<name>.rs` body and nothing
> else.**

Because the numbering is decided up front rather than negotiated, the vocabulary
agent and the body agents run concurrently with no dependency.

**Appending is mandatory.** `FieldOp::ALL` (`field_op.rs:152-177`) is a positional
table and its index *is* the opcode. `mesh_op.rs` states the rule for
`Merge = 12`: *"Appended at 12 rather than inserted, because an opcode is a wire
format: renumbering an existing one silently reinterprets every stored recipe."*

## W0 — serial prep (orchestrator, builds)

1. Verify the WIP quarantine (below).
2. **Split `src/materials/mod.rs`** (1,049 lines) → `materials/library.rs` holding
   the 863-line `LIBRARY`, leaving `mod.rs` as wiring only. It is a `mod.rs` *and*
   a conversion target, which violates one-writer-per-file by construction.
   `src/world/kit/mod.rs` (1,021 lines) is the same shape; it is instead declared
   read-only for this wave.
3. Write `src/characterize/{mod,probes.rs}` and the `#[cfg(test)] mod
   characterize;` line in `lib.rs`.
4. Enumerate every probe case, **including the whole-game witness**.

## W1 — capture (orchestrator, builds)

```sh
RUSTUP_TOOLCHAIN=nightly-x86_64-pc-windows-msvc \
  SHMUP_RECAPTURE=1 cargo test -p axiom-shmup --lib characterize
```

Commit `apps/axiom-shmup/tests/golden/**` with an explicit pathspec. Record the
wall-clock; if the world case dominates, mark it `#[ignore]`-by-default and run it
only in integration.

## W2a — app fan-out (~34 agents, build-free)

Opens with a **single-agent pilot on `fx/impacts.rs`** whose measured before/after
decides whether the wave widens, narrows, or stops. `ax shape --vocab` measures it
at 1,352 code lines, 0.76 literals/line, 0.061 branches/line, over 41 callees
across 435 call sites:

```
129 range   60 float   36 Some   27 reset_spawn   25 round   23 cone
 22 emit_lit   7 reflect   5 toward_hemi   4 blackbody
13 local fns: concrete flesh foliage glass metal plaster water wood
              ground soft spark bullet_hole default
```

`reset_spawn` appears exactly 27 times — once per burst — over 13 named impact
materials, at ~50 lines per burst of which ~38 are numbers. `range` + `float` =
**189 ordered draws in one file**, which is blocker 1 stated as a number.

Then, in parallel:

| slice | files | agents |
|---|---|---|
| `fx` spawn recipes | `muzzle.rs`, `explosions.rs`, `shells.rs`, `ambience.rs`, `atlas.rs`, `lights.rs`+`haze.rs` | 5 |
| `weapons/parts` | `barrel`, `controls`, `hardware`, `magazine`, `receiver`, `optics/*` | 5 |
| `weapons/models` | `pistol`+`rifle`+`smg` (they share `models/mod.rs`) | 1 |
| `weapons` top | `hands.rs`, `viewmodel.rs`, `materials.rs`, `defs.rs`+`rig_math.rs` | 4 |
| `ai` | `parts.rs`, `soldier.rs`, `textures.rs`, `clips.rs`, `weapon.rs`+`animator.rs` | 5 |
| `audio` | `weapons.rs`, `ambience.rs`, `vox.rs` (`foley.rs` already converted) | 3 |
| `world/props` | `containers`+`services`+`signage`+`vehicles`; `cover`+`debris`+`vegetation`+`furniture` | 2 |
| `world/dressing` | `lines`+`palms`+`lamps`+`tyres`; `stalls`+`wrecks`+`rubble`+`sandbags` | 2 |
| `world` top | `layout.rs` (reuse 39.0, branch density 0.000) | 1 |
| `materials` | `library.rs` (the W0 split-out) | 1 |
| `sky` | `system.rs`; `atmosphere.rs`+`dome.rs`+`stars.rs` | 2 |
| `player` | `tuning.rs` | 1 |
| `scene` | `wiring/fx_draw.rs` | 1 |
| | | **33 + pilot** |

**34 is the honest ceiling, not a target to beat.** It is set by one-writer-per-file
over 72 data-verdict files plus coherent grouping. Pushing past it means either
splitting files (churn the integration pass pays for) or two writers per file —
the failure mode this whole playbook exists to prevent.

## W2b — engine vocabulary (3–5 agents, builds, own worktree)

Runs in a **separate git worktree with its own `CARGO_TARGET_DIR`**, concurrent
with W2a. It is not a prerequisite for anything in W2a — the app names
`RecipeGraph`/`MeshOp`/`FieldOp` zero times.

Contents in `02-engine-ops.md`; substrate in `05-runner-substrate.md`.

## W3 — integration (orchestrator, serial)

`RUSTUP_TOOLCHAIN=nightly-x86_64-pc-windows-msvc` on every command. The app's
cdylib exceeds the gnu linker's 65535 export-ordinal limit, so on the default
toolchain nothing in `apps/axiom-shmup/tests/` runs at all.

1. **Wire.** Apply every `mod.rs`/`lib.rs` line from the agent reports. One
   pathspec commit per wire batch.
2. **`cargo check -p axiom-shmup`** until it builds. Expect duplicate helper
   definitions (several agents will independently invent a `Slot` enum), seam type
   mismatches, unused-import churn. **Do not unify the duplicated `Slot` types
   yet** — that is a refactor, and it changes code not yet proved correct.
3. **`cargo test -p axiom-shmup --lib`**, redirected to a file. Piping to `tail`
   discards the exit status.
4. **Triage in this order, and the order is the point:**
   - **The whole-game witness first.** If it fails and every per-recipe case
     passes, you have a stream shift: some agent changed a draw count in a recipe
     the per-case coverage did not reach. Find it by bisecting the agent commits
     against that one test, not by reading code.
   - **Then failures with a differing `<count>`.** Definitive: the driver emits a
     different number of emissions. Almost always an eager/lazy default or a
     missing band.
   - **Then matching count, differing digest.** Open the witness `.hex` and diff
     slot by slot. One slot = a literal transcription error; a whole stride = the
     wrong emit pool; every slot from index *k* onward = a draw-order shift inside
     the burst.
   - **For each, ask which side is wrong.** The ledger is almost always right — it
     was captured from the code being replaced. But a probe with a bad seed is
     possible, and `06-parallel-port-plan.md:108` records "your comparator can be
     the bug."
5. **Revert, do not repair,** any slice that fails and is not obvious quickly.
   `git checkout` the agent's files; the slice goes to W4. A half-fixed table is
   the worst artefact this programme can produce: it compiles, it passes the count
   assertion, and it is wrong.
6. **`cargo test -p axiom-shmup`** and the port goldens: `core_port`,
   `physics_port`, `player_port`, `weapons_port`, `weapons_geometry_port`,
   `weapons_clips_port`, `weapons_mathx_port`, `materials_noise_port`,
   `render_probe_port`, `player_feel`. These are the port's own oracle and
   datafication must not move any of them.
7. **`cargo xtask check-architecture`**; then `cargo dylint --all --
   --all-targets` and confirm the finding count has not risen. **Never run two
   gates at once** — dylint reports a phantom `cargo metadata` error and masks the
   real finding.
8. **Merge the W2b worktree**, then re-run 2–7. Engine ops are measured per-crate
   (`cargo llvm-cov clean --workspace`, then `cargo llvm-cov -p <crate>`); the
   workspace gate cannot complete for reasons predating this programme.
9. **Serve and screenshot.** `uv run scripts/localhost_servers.py start-app shmup`,
   then `scripts/playwright_controller.py`, and **read the image**. A green build
   and a painted street are different facts, and this programme's failure mode is
   a plausible-looking wrong frame.

**Known reds, so they are not mistaken for this work:**
`scene::wiring::weapons::tests::holding_the_trigger_drains_the_magazine_and_kicks_the_camera`
(red on a pristine tree, both toolchains); `axiom-gpu-backend` (~60 failures from
another session's WGSL WIP); `axiom-end-zone` (6 harnesses).

## W4 — second fan-out (~8–12 agents, build-free)

Recipes W2a reported as un-probed, and slices W3 reverted. Requires a W3.5 capture
extension first.

## WIP quarantine (verified at programme start)

Another session is live in this checkout. **Never `git add -A`.** Explicit
pathspecs only.

- `apps/axiom-shmup/src/materials/wgsl/` — 6 modified + 23 untracked `.wgsl`
- `tools/axiom-atlas/src/` — 6 modified + 4 untracked
- `modules/axiom-gpu-backend/src/gpu_backend_api/mod.rs`
- `CLAUDE.md`, `Cargo.lock`, `.gitattributes`, `apps/axiom-shmup/web/index.html`
- `apps/axiom-shmup/tests/materials_gpu_bake_port.rs`,
  `apps/axiom-shmup/tests/materials_wgsl_validates.rs`
- `scripts/capability_sweep.py`, `docs/audits/atlas-ledger-audit.md`
