# The capture harness

## Why it exists

The port had an external oracle: run the original JavaScript under Node and
compare. **Datafication has no external oracle** — the "before" behaviour *is* the
current Rust, and the agents converting it cannot build.

So the oracle is enumerated and frozen up front, and the wave's scope is defined
*by* the oracle: an agent may only convert a recipe the ledger already covers.

## Shape

```
apps/axiom-shmup/src/characterize/mod.rs        the ledger reader/writer   (orchestrator only)
apps/axiom-shmup/src/characterize/probes.rs     the frozen case list       (orchestrator only)
apps/axiom-shmup/tests/golden/<area>.ledger     one line per (case, channel)
apps/axiom-shmup/tests/golden/witness/<case>.hex  bounded raw dumps
```

`characterize` is `#[cfg(test)] mod characterize;` in `src/lib.rs`, so it never
enters the cdylib and cannot worsen the export-ordinal problem.

**The directory is `golden/`, singular.** `.gitattributes:12-13` already carries
`**/tests/golden/** binary` and `**/tests/**/golden/** binary`, so CRLF
corruption is impossible with **no edit to a shared file**. Naming it `goldens/`
would require editing `.gitattributes` — exactly the class of shared file the
fan-out bans.

## What a case is

A deterministic invocation of a recipe with pinned arguments and a pinned seed.
Its fingerprint is **everything observable**, not just the thing being converted.

| subsystem | driver | channels fingerprinted |
|---|---|---|
| `fx` | `FxSystem::test_instance(seed)`, then the recipe | `add`, `lit`, `motes`, `view_add`, `view_lit` raw buffers (`src/fx/particles.rs:263`); decal `raw_positions/normals/uvs/decal_meta` (`src/fx/decals.rs:212-226`); light pool; `fx.rng.state()` **after** |
| `world` | `WorldSystem::init_observed_with_clutter` (`src/world/system.rs:325`) | the ordered `(name, [u32;4])` checkpoint list; digests of `finalize()`'s statics / instanced / collision |
| `weapons` | each model + part with fixed opts | per-`Assembly` digest of `Geo{pos,normal,uv,index}` (`src/weapons/geometry/geo.rs:24`) |
| `ai` | `parts`/`geo`/`soldier` builders; `clips` sampled at fixed t | mesh digests, pose arrays, `rng.state()` after |
| `audio` | each voice recipe into a fresh `AudioGraph` | `NodeRecord` list digest (`src/audio/graph.rs:327`), `rng.state()` after |
| `materials` | each `LIBRARY` entry's `BakeParams`/`MatParams` | **struct-field digest only** — never a full bake (15.5 µs/texel × 1024² is minutes) |
| `ui` | each widget's draw-list build with fixed state | draw-list digest |

## The line format

```
<case-name> <channel> <count> <hex64>
```

`<count>` is there deliberately. A digest tells you *something* moved; a count
tells you *how many emissions* moved. `shmup-port/06-parallel-port-plan.md:107`
already records "a matching count is not proof" — the converse, that a differing
count is definitive, is what makes triage fast.

## The digest

`axiom_kernel::StableHash::of_bytes` over a canonical little-endian encoding:
every `f32` and `f64` as its IEEE **bit pattern**, every slice length-prefixed.

`axiom-kernel` is already a dependency of `axiom-shmup` (`Cargo.toml:28`) — no
`Cargo.toml` edit, which matters because agents may not touch it.
`crates/axiom-mesh/src/mesh_digest.rs` is the precedent for the discipline (bit
patterns, `-0.0 ≠ +0.0`, presence in the encoding), not a dependency to add.

Hashing bit patterns is what makes this survive `-0.0` and `NaN` — the trap
`06-parallel-port-plan.md:162` learned the expensive way.

## The witness dumps

A digest that fails tells you nothing about *which slot* moved. So for a bounded
witness set — **the first two emissions of every burst, and every emission of any
burst with ≤ 4 emissions** — the ledger also writes the full raw buffer as hex
words to `tests/golden/witness/<case>.hex`.

That is the `tracers.rs` "all 96 buffer values" proof, generalised, and it is
bounded: the world's 585,630 triangles are digested, never dumped.

## How a build-free agent writes a test against it

Verbatim — this is the whole mechanism:

```rust
#[cfg(test)]
mod tests {
    use crate::characterize::{Ledger, probe};

    #[test]
    fn the_table_emits_exactly_what_the_hand_written_version_did() {
        let ledger = Ledger::area("fx");            // include_str! of tests/golden/fx.ledger
        probe::fx::impact_concrete().assert_matches(&ledger);
    }
}
```

`probe::fx::impact_concrete()` is written by the orchestrator **before** the
fan-out and is frozen. The agent does not invent the probe, does not invent the
expected values, and does not hand-copy a number.

`Ledger::area` is an `include_str!`: no runtime file IO, no working-directory
dependence, and a missing golden is a **compile error** rather than a silent skip.
Because nobody adds a probe, nobody edits `probes.rs` — the last shared-file
collision is gone.

## The whole-game witness — the single most valuable line in the ledger

`src/scene/game.rs` has `the_frame_is_deterministic_for_the_same_inputs` (a
self-comparison, which proves nothing about drift) and
`one_frame_of_work_per_subsystem_is_pinned` (behavioural thresholds). **Neither is
a fingerprint.**

W0 adds one case: `Game::new_observed(seed)` plus 120 scripted frames, digesting
the camera pose, the movement position, every particle pool, and the four RNG
state words.

A per-recipe golden catches a local error. **Only this catches a stream shift
caused by agent A that surfaces in agent B's subsystem** — the failure class this
programme has and the port did not. It will be the test that fails most often and
localises worst, and it is worth every bit of that.

## Regeneration

**Forbidden to agents.** Datafication is byte-identical by definition; a
conversion that legitimately changes the ledger is a conversion that is wrong.
Regeneration requires a written justification from the orchestrator.

After the programme the ledger is a permanent characterization suite costing one
`include_str!` per area, catching every future accidental reordering. That is a
good deal, not a burden — it is a few thousand lines of text.
