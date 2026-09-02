# Shmup datafication — manifest

`apps/axiom-shmup` is **77,073 code lines** (tests and comments excluded, measured
with `ax shape`) whose entire contact with Axiom is ~120 symbol references. It is a
game built *beside* the engine, not on it.

The predecessor programme — `docs/work-manifests/shmup-promotion/` — lifts the
~21,800 lines of genuine engine capability into `crates/`/`modules/`. That is a
different programme with a different gate (Branchless + Coverage) and it is not
this one.

**This programme's question is narrower:** of what remains, how much is *data
written in Rust* because the engine had no vocabulary to say it any other way?

## What this programme promises

**8–12k lines, not 77k.** That number is deliberate and it is smaller than the
first draft's.

`docs/work-manifests/shmup-promotion/00-manifest.md` already partitions the app:
~21,800 lines are capability the engine lacks (→ Rust in `crates`/`modules`),
~13,500 duplicate engine capability (→ deleted), ~93,000 are this game's content.
Of that content, what a table-and-driver can actually eat is scene composition and
content tables. The rest — `ai/system.rs` (2,668), `ai/agent.rs` (1,851),
`ui/system.rs` (1,884), `fx/system.rs` (2,105) — is decision logic, and
`docs/engine-datafication.md` §3 says decision logic stays Rust.

Promising the 77k is how this programme ends up growing a behaviour VM to make the
number true. **Promise the 10k.**

`ax shape` data-verdict code lines, per directory:

```
weapons 8208   ai 4649   fx 3240   materials 3119   audio 2721
world   2266   sky 1367  scene 948  player 572   physics 456   render 229
```

## Three findings that reshaped this manifest

### 1. The app names the engine's op vocabulary zero times

```console
$ scripts/ax q 'RecipeGraph|MeshOp|FieldOp' --path apps/axiom-shmup/src/
apps/axiom-shmup/src/materials/wgsl/mod.rs:30:   (a doc comment)
```

The proven conversion — `apps/axiom-shmup/src/fx/tracers.rs` — is a plain
`const SPRITES: [Sprite; 3]` and a `for` loop. So is the already-converted
`audio/foley.rs` (`IMPACT`/`STEP`, indexed by `Surface::ALL`). **Neither needs an
engine op.**

So "grow the op vocabulary, then convert the app" describes two independent
programmes, not a pipeline. Sequencing the fan-out behind gated engine work would
idle ~34 build-free agents behind a Coverage-Law crawl for no gain. Engine work
still leads — it starts first and it is where the durable value is — but it runs
in its own worktree and **does not block** the app track.

### 2. `materials/surfaces/` is out of scope, and the manifest that said so was right

`crates/axiom-recipe/src/recipe_graph.rs:18` sets `MAX_NODES = 256`; the inlined
generator graphs are 2.1k–43.4k nodes. `crates/axiom-field/src/field_op.rs` has 27
ops with no `Floor`/`Fract`/`Mod`. Adding those three does not close a
43,000-node gap. Those 1,799 lines are not in this programme; hand-written WGSL is
the route and it is already being taken by another session.

### 3. This manifest's predecessor proposed a burst schema with a determinism bug

`shmup-promotion/00-manifest.md:466` says "a burst is a table row of
`Range { lo, hi }` (a constant being `lo == hi`)". **That is wrong**, and it is
wrong in the way that is hardest to see:

- a driver calling `rng.range(lo, hi)` per slot **consumes a draw for every
  constant**, shifting the shared stream;
- the inverse special case — "skip the draw when `lo == hi`" — silently
  mis-handles a genuine `rng.range(x, x)` in the source.

The slot type must be three-valued. See `01-agent-brief.md`.

`apps/axiom-shmup/src/fx/impacts.rs:284` is the case that proves it:

```rust
s.delay = if band == 0 { 0.0 } else { fx.rng.range(0.02, if band == 1 { 0.09 } else { 0.2 }) };
```

A conditionally-skipped draw, inside a banded loop, whose upper bound is itself
band-dependent.

## Blockers — verified with `ax`, each gating real lines

1. **RNG draw order is the level's identity.** `world/system.rs` and
   `scene/level.rs` pin 17,133 ordered `Assembler::add` entries. `Assembler::muted`
   (`src/world/assembler.rs:364`) exists *solely* so a removed set-piece still
   consumes its draws. `RecipeGraph` keys entropy per node from a positional
   address (`crates/axiom-proc-core/src/proc_core.rs:57`) and structurally cannot
   express a shared ordered stream. **Resolution: it never will — see §4 below.**
2. **No triangle-mesh collider or BVH in the spine.** Attaches as
   `PhysicsShapeKind::TriangleMesh = 5` (the enum is table-ordered `Sphere=0 …
   Heightfield=4`, so appending is safe). Belongs to the *promotion* programme.
3. **No `MeshOp` writes the colour stream** added this cycle. Needs
   `MeshOp::PaintColor`; precedent is `Displace` binding a `FieldGraph`.
4. **`DVec3::normalize` is fallible; the sky ports are infallible** and pinned to
   JS goldens that yield `Infinity`/`NaN` on degenerate input. 8 call sites, but a
   semantics change. Needs `normalize_or_zero`.
5. **`ProcCore::execute` takes `F: Fn`** (`proc_core.rs:40`). A scene evaluator
   mutates `RunningApp`, so it needs `FnMut`. ~2 lines. Blocks the runner.
6. **No `.axpkg`, no `axiom-appc`, no `axiom-runner`** — and
   `docs/engine-datafication.md` cites three artifacts that do not exist (§7).

## No new modules

The first draft proposed `axiom-nav`, `axiom-ragdoll` and `axiom-atmosphere`.
Checking the repo killed all three; each would have been a ceremonial module
sitting next to the thing that already owns the domain.

| capability | proposed | actual home | why |
|---|---|---|---|
| ragdoll | `axiom-ragdoll` | `modules/axiom-physical-animation` | already a feature-module that "binds a humanoid rig to physics bodies… reads back pose frames" |
| weighted pathfinding | `axiom-nav` | `modules/axiom-grid` | already owns `path`; today's BFS-distance-field descent is the unweighted case |
| sky / celestial | `axiom-atmosphere` | `crates/axiom-host::frame_sky` | already "a vertical gradient with an optional celestial body" |

It does not go to `axiom-space` either — that layer is only `Address`/`SpaceApi`.

## Wave table

| wave | contents | depends on | agents | builds? |
|---|---|---|---|---|
| **W0** prep | split `materials/mod.rs`; write `src/characterize/`; enumerate every probe case | — | 1 (orchestrator) | yes |
| **W1** capture | run the harness, commit `tests/golden/**` | W0 | 1 (orchestrator) | yes |
| **W2a** app fan-out | the conversions, one agent per file | **W1 frozen** | ~34 | **no** |
| **W2b** engine vocabulary | `FieldOp::{Floor,Fract,Mod}`, `MeshOp::PaintColor`, the substrate | — (concurrent with W2a) | 3–5 | yes |
| **W3** integration | compile, run, triage | W2a + W2b | 1 | yes |
| **W4** second fan-out | recipes W2a reported as un-probed; slices W3 reverted | W3 | ~8–12 | no |

W0 → W1 → W2a is a hard serial chain. W2b is independent of all of it.

See `04-waves.md` for per-agent assignments and `03-capture-harness.md` for the
ledger.

## The determinism decision

> **Draw-order-dependent code stays as Rust `const` tables driven by a Rust loop,
> in the app. It does not go through `RecipeGraph`.**

Reasons, in the order the evidence forces them:

1. `RecipeGraph` structurally cannot express a shared ordered stream — its entropy
   is per-node and positional.
2. Fixing that means a second entropy model in the engine, order-dependent and so
   unverifiable by the branchless `try_fold` interpreter, motivated by exactly one
   app. `docs/engine-datafication.md` §2 classifies that as `N = 1` datafication,
   which *adds* code.
3. The 256-node budget forecloses it anyway.
4. **The in-app table already delivers the whole benefit.** The Datafication Law's
   saving is `(N−1) × per-variant-code`, and it does not care whether the table is
   a `const [Row; N]` in Rust or a TOML file. A Rust `const` table costs zero
   interpreter, zero format, zero serializer.

What *is* worth expressing as data about draw order is the **checkpoint list** —
`WorldSystem::init_observed_with_clutter`'s `(name, [u32;4])` sequence
(`src/world/system.rs:325`), committed to the ledger as the shared stream's
contract. Build that; do not build a draw-order VM.

## Status

See `99-status.md`.
