# `axiom-proc` — architecture

The procedural-generation **graph core**: a recipe DAG evaluated deterministically
into a neutral artifact + trace. Everything in Phases 7–12 builds on it.

## What it is

- **`Recipe`** — a versioned DAG of generic, domain-free nodes, built by append
  (`const_node`/`draw`/`add`/`xor`), each returning the new node's index. Inputs
  reference only earlier nodes, so it is a DAG by construction.
- **`ProcApi`** — validates the recipe, keys an `entropy` stream by
  `(seed, Address, recipe.version)`, and evaluates it. An invalid recipe is
  rejected as data (`None`), never a panic.
- **`Artifact`** — the neutral output: opaque `u64` words + the recipe's generator
  version, with canonical bytes and a stable `StableHash` digest.
- **`ProcTrace`** — the decision log (`(op_code, value)` per node), serialized and
  digested like the artifact so the two boundaries golden-compare independently.
- **`Evaluation`** — a resumable, **budget-independent** run: stepping one node at
  a time yields byte-identical output to one whole evaluation.

It generalizes `apps/axiom-growth`'s app-local `Stage`/`StageRegistry`/`Pipeline`
into one engine substrate.

## Why branchless graph evaluation

The roadmap flagged this as the hard part. Node dispatch is a **table index over
the fieldless op discriminant** (`OPS[node.op as usize](…)`), never a `match` over
kinds; validity is an `iter().enumerate().all(…)` predicate; the per-step body is
an `Option::into_iter().for_each(…)` so a finished evaluation skips it. There is no
control flow in the spine — the evaluator is data transforms end to end.

## Why it depends on kernel + space + entropy (and not math)

- **kernel** — `StableHash` (artifact/trace digest), `BinaryWriter` + `SchemaVersion`
  (canonical, versioned serialization).
- **space** — the `Address` a recipe is evaluated *at* (and the spatial component
  of the entropy key).
- **entropy** — the `(seed, address, version)`-keyed stream the `draw` op pulls
  from.
- **math is intentionally absent.** Evaluation references no geometry. Declaring
  `math` would be a ceremonial dependency the `engine_genuine_dependency` dylint
  bans; if a future node genuinely needs geometry it is added *then*, by use.

## What does **not** belong here

- **No domain content.** No terrain, biome, mesh, level, or **noise** — the
  artifact is opaque words. Domain meaning is a Phase 9 *module*'s job, consuming
  `ProcApi`; a module can never add node kinds to this layer.
- **No unbounded work.** Evaluation is budgeted/resumable; nothing runs to
  completion inside a frame against the caller's will.
- Browser/platform APIs, randomness (it routes `entropy`, which routes the kernel
  RNG), wall-clock time.

## The invariants it guarantees

- **Deterministic:** the same `(recipe, seed, address)` yields byte-identical
  artifact + trace (proven by byte equality; the digest only indexes).
- **DAG-validated:** a forward/self/back reference is rejected as data.
- **Budget-independent:** incremental evaluation == whole evaluation, byte for byte.
- **Versioned:** bumping the recipe version re-keys the stream and changes the
  artifact; restoring it restores the bytes.
