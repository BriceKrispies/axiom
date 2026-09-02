# Status

Newest first. One row per landed change.

## Landed and pushed

| what | where | proof |
|---|---|---|
| f64 geometry family | `crates/axiom-math`: `DVec2`, `DVec3`, `DAabb`, `DTriangle`, `DSegment`, `DClosestPair` | 100% per-crate coverage; `to_single`/`from_single` is the one named narrowing boundary |
| scalar numerics | `crates/axiom-math`: `hypot`, `hypot3`, `round_ties_up`, `signum_with_zero`, `nonzero_or` | promoted out of the app's `jsmath.rs` |
| the scalar policy | `crates/axiom-math/src/scalar.rs` | rewritten: f32 is the **interchange** scalar; evaluate at the precision the domain requires, narrow once at a named boundary |
| f64 in the kernel | `binary_writer`, `binary_reader`, `reflect` | `write_f64`/`read_f64`, `impl_reflect_scalar!(f64, …)` |
| two noise bases | `crates/axiom-noise`: `unit_noise`, `hash_01`, `value_noise_01`, `value_fbm_01`, `signed_noise`, `permutation_lattice`, `perlin_2d`, `cellular_2d` | `tests/positional_basis_golden.rs`, Node-captured, `assert_eq!` not tolerance |
| `MeshOp::Merge = 12`, `MeshOp::Trs = 13` | `crates/axiom-proc-mesh` | appended, never inserted — an opcode is a wire format |
| a colour stream on `MeshBuffer` | `crates/axiom-proc-mesh/src/mesh_buffer.rs` | `colors`, `with_colors`, `without_colors`, `has_colors`, and `respecified()` which carries colour + skin forward |
| every vertex-preserving op rebuilt through `respecified` | `crates/axiom-proc-mesh/src/transforms.rs` | `from_parts` produces an *uncoloured* mesh, so `transform`/`bevel`/`bend`/`uv_project`/`displace`/`triangulate` would each have silently dropped an authored channel |
| `ax shape` | `tools/axiom-atlas/src/shape.rs` | walks the real AST with `syn`; reports code lines, literal density, branch density, reuse, AST node count, and a data/mixed/algorithm verdict; `--vocab` names the closed vocabulary |
| the first conversion | `apps/axiom-shmup/src/fx/tracers.rs` | `const SPRITES: [Sprite; 3]` + driver, pinned by `EXPECTED: [f32; 96]` — every field of every sprite including the three RNG seed draws |
| app bindings, algorithm gone | `world/noise.rs` 258→107, `jsmath.rs` 328→120, `physics/math.rs` 706→456, `fx/noise.rs` 310→205 | constants kept, algorithm promoted |

### Defects found and fixed on the way

- **Ray/AABB missed a box the ray passes through.** The slab test's `0 × ∞ = NaN`
  defeated a comparison-based test — a grounded character's downward probe, every
  frame. Fixed by substituting ∓∞ for NaN bounds. Ray/AABB is the BVH's inner
  loop, so this had to land before the BVH, not after.
- **The wrong `round` got promoted.** `world/noise.rs`'s `round_half_up` is
  `(v + 0.5).floor()`, wrong at `0.49999999999999994`; `jsmath` had documented
  this and implemented it correctly. Consolidated on `axiom_math::round_ties_up`.
  **The goldens did not move**, because the pathological input never arose — which
  is exactly how a latent defect survives a golden suite.
- Two of my own test assertions were wrong, not the code:
  `adjacent_lattice_cells_decorrelate` asserted a pointwise property a uniform
  hash does not guarantee (rewritten as statistical, E|U₁−U₂| = 1/3), and
  `one_cube()` used TRS scale `(0,0,0)`, collapsing the reference mesh.

## Not started

W0 (prep), W1 (capture), W2a (app fan-out), W2b (engine vocabulary + substrate),
W3, W4. See `04-waves.md`.

## Corrections this programme made to its own predecessor

- **`shmup-promotion/00-manifest.md:466`'s burst schema has a determinism bug.**
  `Range { lo, hi }` with "a constant being `lo == hi`" either consumes a draw per
  constant or mis-handles a genuine `rng.range(x, x)`. The slot type is
  three-valued. See `01-agent-brief.md`.
- **`docs/engine-datafication.md` cites three artifacts that do not exist** — the
  rotating-cube TOML, its manifest test, and a runtime TOML loader. There is no
  runtime TOML parsing anywhere in this repo. See `05-runner-substrate.md` §0.
- **The 77k collapse figure was never this programme's to claim.** Promise the
  8–12k. See `00-manifest.md`.
- **Three proposed new modules all had existing homes** (`axiom-ragdoll` →
  `axiom-physical-animation`, `axiom-nav` → `axiom-grid`, `axiom-atmosphere` →
  `axiom-host::frame_sky`). Each would have been a ceremonial module beside the
  thing that already owns the domain.
- **`materials/surfaces/` is infeasible, not high-value.** It was briefed as the
  top target; an agent proved a 256-node budget against 2.1k–43.4k-node graphs and
  the correction stands.
