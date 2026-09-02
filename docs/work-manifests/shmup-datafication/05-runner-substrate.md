# The runner substrate — capability catalog, `.axpkg`, `appc`, runner

## Four corrections to `docs/engine-datafication.md` before anything is built

That document is the governing text for this programme, and it cites three things
that do not exist. Verified:

| claim | where | reality |
|---|---|---|
| "111 lines of TOML (`apps/axiom-gallery/tests/rotating_cube/package/scenes/main.toml`)" | §6 | `apps/axiom-gallery/` is `README.md`, `visual_targets/`, `web/`. No crate, no tests — it went with the gallery de-merge. |
| "`apps/axiom-gallery/tests/rotating_cube_scene_manifest.rs` asserts the data and the imperative Rust agree" | §7 | Gone with the crate. |
| "the `growth` `world_tags` path that actually loads TOML at runtime" | §8 | **There is no runtime TOML parsing anywhere in this repo.** `toml::from_str` appears in exactly three places, all tooling: `crates/xtask/src/*`, `tools/axiom-asset-pack/src/lib.rs:187`, `tools/lints/engine_genuine_dependency/src/lib.rs:104`. |

The fourth is scope: §6 attributes the collapse to "the ~50k app tier". See
`00-manifest.md` — **promise the 10k, not the 77k.**

A correction pass on that document is `repo`-verdict work under the Atlas Friction
Law: a contract written down wrong is worse than one merely implied.

## 1. The capability catalog → `crates/axiom-kernel/src/capability.rs`

**Not a new layer.** CLAUDE.md, in its own words: *"a broadly-shared primitive
(one many layers need but no single adjacent layer 'owns') belongs in the
**kernel**, the shared root every layer may depend on, not wedged into the graph
as its own layer with a manufactured edge."*

`recipe`, `field`, `proc-mesh`, `proc-texture`, the new `package` layer, the
runner module and the compiler tool must all name a capability descriptor; none of
them owns it. A `crates/axiom-capability` layer would be exactly the *"tiny
ceremonial layer just to feel organized"* the Layer Rules ban, and it would split
call-reflection from value-reflection (`TypeSchema`, already in the kernel) across
two tiers.

`CapabilitySchema` is `TypeSchema` for a **call** instead of a **value**, built
from the same static-string discipline: `ParamSlot { name, type_name, words }`,
`CapabilitySchema { name, code, arity, inputs, params, result }`,
`CapabilityDomain`, `CapabilityCatalog { epoch, domains }`.
`VARIADIC = u8::MAX` mirrors `axiom_field::FieldSignature::PARAM_DECIDED_INPUTS`.

**Branchless:** every accessor is a field read; `op()` is two `get`s (the
`OPS.get(i)` idiom at `crates/axiom-proc-mesh/src/dispatch.rs:52`); `find()` is
`iter().position`; `param_words()` is `map().sum()`.

**The coverage trap, stated so the next agent does not re-litigate it.**
`crates/axiom-field/src/signature.rs:112` records it: *"a `const fn` used only by a
`const` initializer is evaluated at compile time and so can never be reached by a
test."* `axiom-field` dodges it with same-crate struct literals, which does not
work for catalogs in other crates. So `ParamSlot::new`/`CapabilitySchema::new` are
public `const fn`, and **the kernel's own tests call them at runtime with ordinary
`let`s.** That is the fix.

**How data names an op with no runtime dispatch** — the whole answer, in one
sentence:

> `axiom-appc` resolves names to `(domain: u16, code: u16)` at compile time. The
> runtime never sees a name, therefore never performs a lookup, therefore never
> dispatches.

At runtime the binding is two `const` index operations. The catalog's strings
exist only in the compiler and in `--emit-catalog`. No `TypeId`, no `Any` — the
`engine_no_runtime_type_branch` dylint stays satisfied by construction.

**No duplication of the existing op tables.** The catalog is *published by* each
domain from its own table, never restated:

| domain | owner | derived from |
|---|---|---|
| `mesh` | `crates/axiom-proc-mesh` | `MeshOp` (14) + `OPS: [MeshOpFn; 14]` |
| `texture` | `crates/axiom-proc-texture` | `TextureOp` (12) + `OPS: [TexOp; 12]` |
| `field` | `crates/axiom-field` | `FieldOp::ALL` (27) + `SIGNATURES` — arity/params come from `op.signature()`, so there is literally one number |
| `scene` | `modules/axiom-runner` | new |

Each domain carries one test asserting `CATALOG.len() == OPS.len()` and
`CATALOG[i].code() == i` — the pin
`crates/axiom-field/src/field_op.rs::codes_are_their_dispatch_indices` already
runs.

This also lets the ~23 inline `p.len() >= N` checks in `proc-mesh`/`proc-texture`
read arity from the catalog row instead of a doc comment: one arity number per op
rather than a table parallel to a table.

## 2. `.axpkg` → `crates/axiom-package` (new layer, `depends_on = ["kernel", "recipe"]`)

**Why a layer and not a module.** Module Law #8: a module's `lib.rs` exposes
exactly one facade plus its id vocabulary. A package format *is* a vocabulary
(`Package`, `PackageBuilder`, `SectionKind`, `RecipeEntry`, `SceneEntry`,
`PackageError`). A module structurally cannot publish it. Both `axiom-appc` (tool)
and `axiom-runner` (module) must name it, so it sits below both.

### The one structural decision: a scene *is* a `RecipeGraph`

Do not invent a second graph encoding. `RecipeGraph` already gives: opcode +
`Param` words + input links; acyclicity by construction (`validate()` enforces
inputs referencing strictly-earlier nodes, `recipe_graph.rs:95`); the node budget;
canonical LE bytes; `digest()`.

A scene is a recipe whose domain tag is `scene`; a cube is a recipe whose domain
tag is `mesh`; a scene node references a mesh recipe **by table index in a
parameter word**, exactly as `MeshOp::Displace` references a `FieldGraph` by
`field_index`.

Consequence: the domain tag rides on the *recipe*, not the node, so `Node::op`
stays a domain-local `u16` and **nothing in `axiom-recipe` changes.**

### On-disk shape

Canonical little-endian through `BinaryWriter`, the same construction as
`RecipeGraph::write_to`.

```
magic            u32   0x4158_504B  ("AXPK")
schema           SchemaVersion
package_id       u64
content_version  u32
catalog_epoch    u32
catalog_digest   u64          diagnostic, not the gate
section_count    u32
section[]: kind u16 | byte_len u32 | payload
```

| kind | section | payload |
|---|---|---|
| 0 | `STRINGS` | every name in the package is an index into this. The runtime never compares a string. |
| 1 | `RECIPES` | `u16 domain` + **`RecipeGraph::write_to` verbatim** |
| 2 | `BLOBS` | each produced by its owning layer's own `serialize()` — `FieldGraph::serialize`, `Surface::serialize`. This layer never re-encodes them. |
| 3 | `ASSETS` | the bytes **`AssetsApi::encode_manifest`** produced, verbatim. One asset-manifest format in this repo, not two. |
| 4 | `SCENES` | `u32 name_string` + `u32 recipe_index` |
| 5 | `ENTRY` | entry scene, fixed step nanos, seed, title, width, height |

Identity: `Package::digest() = StableHash::of_bytes(&self.serialize())` —
character-for-character the construction at `recipe_graph.rs:157`.

### Versioning — three axes, deliberately separate

| axis | carrier | breaks when | effect |
|---|---|---|---|
| container shape | `SchemaVersion` | a section is added/reordered | every package rebuilt; loud by construction |
| op vocabulary | `catalog_epoch: u32` | an op is **renumbered, removed, or has its arity changed** | runner refuses on `!=` |
| content | `content_version: u32` | authored values change | nothing; the digest moves |

**Appending an op is free** — the rule `mesh_op.rs` states for `Merge = 12`. A
catalog *digest* as the compatibility gate would invalidate every package on every
append, which is why the digest is recorded and reported but the **epoch** is the
gate.

## 3. `axiom-appc` → `tools/axiom-appc`

A tool, three-fold: it reads and writes the filesystem (and `modules/axiom-assets`
states the engine-tier discipline — *"**Not** an I/O layer. It performs no fetch,
no network, no disk, no threads"*); it needs `serde` + `toml`, which appear
nowhere in the engine graph; and tools sit outside the coverage and branchless
gates, which a TOML parser — irreducibly branchy and fallible — needs.

**Copy `tools/axiom-asset-pack`'s anti-drift structure verbatim.** Its header
states the rule: *"The manifest binary format is OWNED by that module; this tool
encodes through `AssetsApi::encode_manifest` and verifies its own output
round-trips through `AssetsApi::from_manifest_bytes`, so the two sides can never
drift."* `axiom-appc` encodes through `PackageBuilder`, re-reads through
`Package::deserialize`, and asserts byte-identity **before writing a single file**.

What it validates at compile time: op exists · arity · param word count · slot
types · input kinds (producer's `result()` vs consumer's `inputs()[slot]`) ·
cycle-freedom · the 256-node budget · reference integrity · determinism
(encode → decode → re-encode → byte-identical).

Second mode: **`axiom-appc --emit-catalog`** prints the catalog as markdown. That
discharges Vocabulary Law admission test #5 — *"the spec an author reads is
generated from the engine, so it cannot drift"* — mechanically instead of by
prose.

**`.axpkg` is binary; TOML never reaches the runtime.** The repo has zero runtime
TOML parsers today. Keep it at zero.

## 4. `axiom-runner` → `modules/axiom-runner`, `kind = "feature-module"`

The argument is airtight, and both wrong answers are structurally impossible:

- **Not an engine module** — it must name `RunningApp` from `modules/axiom`, which
  is itself a feature module. Module Law #2: an engine module depending on any
  module is `ModuleDependsOnModule`.
- **Not an app** — *"Apps are the only leaf composition roots. Nothing else may
  depend on an app."* A game is an app. If the runner were an app, no game could
  ever use it, which is the entire point.
- **Not a layer** — a layer's Cargo deps may contain only layer crates (Hard rule
  #1), and the dispatch table's fn pointers take `&mut RunningApp`.

⇒ feature module. `allowed_modules = ["engine"]`.

**`SceneOp` is a `u16` newtype with a `const` catalog, not an enum.**
`tools/lints/engine_no_large_enums` caps an enum at 24 variants and the catalog
will exceed that. The reasoning is already written at `field_op.rs:22-42`: the
fix the lint prescribes (nested sub-enums) resets the count per level, *"which is
exactly wrong here: the dispatch technique needs one flat discriminant space
indexing one `const` table."*

**`SceneValue` is not a data-carrying enum** (that forces a `match`). It is
`{ kind: ValueKind /* u16 */, raw: u64 }` — the `Param` discipline lifted one
level. An `Entity`, a `Handle<Mesh>`, a `Handle<Material>` and a table index all
fit in `u64`. **The runner never checks the kind, because `axiom-appc` already
proved it** — validate and canonicalise at a preparation barrier, execute
branchlessly afterwards.

### The one blocking engine change

`ProcCore::execute` takes `F: Fn(NodeEval<'_, Out>) -> Option<Out>`
(`crates/axiom-proc-core/src/proc_core.rs:40`). A scene evaluator mutates
`RunningApp`, so its closure is `FnMut`.

Two ways out; only one is honest:

- `RefCell<&mut RunningApp>` inside a `Fn` closure. No engine change — and it
  introduces a runtime borrow-panic path the Coverage Law then demands a test for.
  **Rejected**: an untestable-or-contrived branch is the symptom the Coverage Law
  calls a design signal.
- **Widen `F: Fn` → `F: FnMut`.** Source-compatible for every existing caller
  (every `Fn` is an `FnMut`); needs `let mut eval = eval;` and nothing else; stays
  branchless; `try_fold`'s closure is already `FnMut`.

Take the second. ~2 lines plus one test, and it is **step 0**.

### The leaf app

`apps/axiom-play` — a thin composition root owning the wasm entry, the canvas, and
`WindowingApi::run_web_multi`. Module Law #9 keeps browser APIs to
`host`/`windowing`, and `apps/axiom-shmup`, `apps/burnt-rubber` and `apps/dog`
already drive `run_web_multi` from the app tier *"because `run` exposes no
per-frame app hook"*. ~150 lines. It is a host, not a game.

## 5. f32 / f64 — already settled; do not re-open it

`crates/axiom-math/src/scalar.rs:7-24` carries the ratified policy: *"Axiom
standardises on IEEE-754 `f32` as the engine's **interchange** scalar … Evaluate
at the precision the domain requires; narrow once, at the boundary."* With the
measurement behind it (`axiom_surface::srgb_to_linear`, all 256 byte inputs):

| how the curve is computed | values differing | worst gap |
|---|---|---|
| f64 throughout | 254 / 256 | 1.08e-11 |
| **f64, then narrowed to the f32 uniform** | **0 / 256** | **0** |
| natively in f32 | 175 / 256 | 1.79e-7 |

And the pressure is real: shmup measures 8,635 `f64` occurrences against 3,365
`f32`; `src/audio/system.rs:91` declares `pub type Vec3 = [f64; 3]`.

**Option A — widen `Param` to `u64`.** The wire format is `writer.write_u32(p.bits())`
(`crates/axiom-recipe/src/node.rs:47`), so every stored recipe and every digest
moves: `axiom-placement`, `axiom-levelgen`, `axiom-worldsave`, the quintet
trajectory, the playground digest — all five re-goldened once already — plus
`axiom-field`'s eval goldens and its CPU↔GPU parity corpus. It breaks three
encodings *defined* as `u32` word counts (`FieldOp::Const`'s `[type,x,y,z,w]`,
`Transform`'s four `Vec4` columns, `SEED_WORDS = 2`). And it buys nothing where
recipes are used at scale, because `axiom-field` compiles to WGSL and **WGSL has
no `f64`**. Highest cost, lowest benefit. **Reject.**

**Option C — accept divergence and re-pin.** Throws away the port's only proof
that it computes the same function as its reference, and the table above says
f32-native does not lose digits, it *introduces* disagreement on 175/256 inputs.
On audio it is worse than a golden problem: ~1e-7 relative on a schedule time is
~6e-5 s at 600 s, and accumulated oscillator phase drifts audibly. **Reject.**

**Option B — an f64 param, untagged.** In its *tagged* form this is also wrong:
`crates/axiom-recipe/src/value.rs:3-8` says a `Param` is *"deliberately untyped in
the graph so the container stays domain-free and branchless (no per-variant
`match` to read a value)."* A tag reintroduces exactly that.

In its **untagged slot-pair** form it costs nothing and already has a precedent in
the tree: `FieldOp::Noise` carries a `u64` seed as two consecutive `u32` words
(`SEED_WORDS`, `crates/axiom-field/src/noise_words.rs`). An op knows its own slot
layout; it reads two words and recombines. No tag, no branch, no wire change.

### Recommendation: B, untagged slot-pair, declared in the catalog

1. `crates/axiom-recipe/src/value.rs` gains the named widening boundary — the
   mirror of `DVec3::to_single`/`from_single`, introduced precisely so *"'compute
   in f64, narrow once' is a symbol you can search for rather than an `as f32`
   scattered across call sites"*:
   ```rust
   impl Param {
       pub fn pair(value: f64) -> [Param; 2];   // to_bits, lo then hi
       pub fn from_pair(pair: [Param; 2]) -> f64;
   }
   ```
   Two functions. Branchless. Trivially covered. **Zero wire change, zero digest
   movement.**
2. `ParamSlot` carries `words: u8`. A slot declared `("start_time", "f64", 2)`
   occupies two words; `param_words()` is the sum; `axiom-appc` validates against
   it.
3. **`axiom-field` never declares a 2-word slot.** The field algebra stays
   f32-pure and its WGSL parity is untouched by construction rather than by
   discipline.

What this does *not* do: it does not make the interior of the audio DSP, the BVH,
the character controller or the atmosphere LUTs into data. Those are `N = 1`
algorithms — §2 says datafying them *adds* code. Only their **tunables** become
slots.

## 6. The minimum end-to-end proof

Rebuild the rotating cube, inverted: **the data is the source, the Rust is the
check.**

Content at `apps/axiom-play/content/rotating_cube/`: `axiom.app.toml`,
`scenes/main.toml`, `meshes/cube.toml`. The mesh recipe is not garnish — it forces
`DOMAINS[]` to have ≥2 real entries and proves `ProcCore` *drives* the substrate
rather than being cited by it. The cube's geometry comes from the package, **not**
from `Mesh::cube()`.

Minimum scene catalog — 10 ops, small enough to reach 100% coverage, large enough
to prove the shape: `mesh_from_recipe` · `material_const` · `spawn` ·
`set_transform` · `set_parent` · `add_light` · `set_camera` · `set_clear_color` ·
`spin` · `set_ambient`. Each maps to an existing `RunningApp` method
(`modules/axiom/src/app/{authoring,queries,render_look}.rs`).

Three assertions in `apps/axiom-play/tests/`:

1. **Compiler determinism.** `axiom-appc` on the TOML produces bytes byte-identical
   to the committed golden `tests/golden/rotating_cube.axpkg` — `binary` by the
   existing `.gitattributes` rule.
2. **Format determinism.** `deserialize(golden)` → `serialize()` byte-identical,
   and `digest()` matches a pinned `StableHash`.
3. **The inverted mirror.** `RunnerApi::load(&package)` then `tick(n)` produces a
   `FrameOutcome` byte-identical to the same scene authored imperatively in Rust
   in the same test file, at ticks 0, 1 and 60; tick-N replayed twice is
   byte-equal; tick-N and tick-N+60 differ. Those are CLAUDE.md's own "What the
   rotating cube slice proves" criteria, reused without amendment.

## 7. Build sequence

| # | step | depends on | notes |
|---|---|---|---|
| 0 | `ProcCore::execute`: `F: Fn` → `F: FnMut` | — | ~2 lines + 1 test. Blocks 4. Do it first and alone. |
| 1 | kernel `capability.rs` | 0 | land **with** 2 — a caller-less export is the ceremonial-export failure |
| 2 | `MeshOp`/`TextureOp`/`FieldOp` publish catalogs; arity checks read the row | 1 | byte-identical output; no consumer edits |
| 3 | `crates/axiom-package` | 1 | land **with** 4, same reason |
| 4 | `modules/axiom-runner` — `SceneOp` (10 ops), `OPS`/`DOMAINS`, `RunnerApi` | 0,1,2,3 | facade + `pub use ids::{SceneOp, ValueKind}`; errors are `PackageResult`, no third export |
| 5 | `tools/axiom-appc` + `--emit-catalog` | 1,3,4 | asset-pack's anti-drift structure verbatim |
| 6 | `apps/axiom-play` + content + the three goldens | 4,5 | the proof |
| 7 | correct §6/§8 of `docs/engine-datafication.md` | 6 | replace the three dead citations with the live slice |
| 8 | *only then* widen the catalog, one op at a time | 6 | each op needs ≥2 unrelated games that cannot be expressed without it |

Steps 1+2 and 3+4 land as single changes so no layer ever ships an export with no
caller.

Every name is written literally — no `concat!`/`paste!` assembly — so `ax def
SceneOp`, `ax impact CapabilitySchema` and `ax owns` work from day one. That is a
hard requirement: *"A symbol is unsearchable because its name is assembled … →
`repo`."*

## 8. Bad ideas, said plainly

- **Do not put a rule/behaviour engine in `.axpkg`.** The moment a package can
  express `on_hit → subtract health`, it is the reflective VM §5 bans wearing a
  data costume.
- **Do not auto-generate the scene catalog from `RunningApp`'s ~40 public
  methods.** They exist because apps needed them, not because they are vocabulary.
  A catalog that mirrors an API is a junk drawer with a schema.
- **Do not raise `MAX_NODES = 256`.** A level with 5,000 objects is not 5,000
  nodes — `axiom-placement`'s two-node recipe is the pattern, and the object count
  is a parameter word.
- **Do not make `.axpkg` a zip, and do not compress it.** A canonical byte stream
  is the determinism proof; a compressor with version-dependent output makes the
  digest a lie.
- **Do not add a runtime TOML parser, ever.**
- **Do not do §7 #1 (scene schedule → data) first.** The runner drives
  `RunningApp::tick`, which already owns its schedule. Doing it first is one
  production file of churn at net ≈ 0 LOC, and it delays the only thing that
  proves any of this works.
- **Do not build `crates/axiom-capability`.** It is the ceremonial layer CLAUDE.md
  names by hand.
