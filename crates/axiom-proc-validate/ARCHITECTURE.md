# `axiom-proc-validate` — architecture

Makes generated output **trustworthy**: validation verdicts, scoring, and
bounded repair over a generation's neutral output words.

## What it is

- **`Constraint`** — a generic, domain-free check over a word list:
  `min_count(n)`, `max_value(v)`, `non_zero()`. A fieldless `ConstraintKind`
  + a threshold, dispatched through eval/repair tables (branchless).
- **`ProcValidateApi::validate(&[u64], &[Constraint])`** — a deterministic
  [`ValidationReport`]: a `(kind_code, satisfied, score)` verdict per constraint,
  whether all passed, and the total score. Pure in the words.
- **`ProcValidateApi::repair(&[u64], &[Constraint])`** — a single **bounded**
  pass of word-level fixes (clamp to a max, lift off zero) that returns a new,
  re-validatable `Vec<u64>`.
- **`ValidationReport`** — serializable (canonical bytes) + a stable `StableHash`
  digest, so reports golden-compare independently of what produced the words.
- **`sample_until_valid`** — the generative counterpart: bounded, branchless
  rejection sampling.

## Why it depends on the kernel, and on nothing else

- **kernel** — `StableHash` + `BinaryWriter` for the report's canonical bytes and
  digest.

That is the whole dependency list. Until manifest **P1** this layer took the v1
`axiom_proc::Artifact` — a `(generator_version, Vec<u64>)` container — and it
declared `proc` for that one type. The surviving recipe stack (`axiom-recipe` +
`axiom-proc-core`) is deliberately **generic over its output type** and owns no
artifact container, so there is nothing there for validation to name: a verdict
over neutral words needs no graph, no executor, and no entropy. Declaring
`recipe` or `proc-core` here to keep the layer looking like part of the
generation cluster would be precisely the ceremonial dependency the Layer Law
bans, so this is a `kernel`-only layer, like `recipe` itself.

The words a caller passes are whatever a generator produced — the `Vec<u64>`
output of a `ProcCore::execute` run, a module's own neutral output, or a golden
read back from disk.

## What does **not** belong here

- **No domain rules.** "Rivers must reach the sea", "a level is solvable", "a room
  has a door" are *domain module* concerns (Phase 9), consuming this layer's
  generic constraint vocabulary — never baked in here.
- **No generation container.** Re-introducing an artifact struct would recreate
  the coupling P1 removed, and it would have to live in a lower layer to be
  shared, which is exactly what `axiom-recipe` refused to do.
- **No unbounded repair.** Repair is one pass, not a loop-to-fixpoint, and it
  never invents content: a structural minimum-count failure is left unsatisfied by
  design (a documented, honest limit).
- **No generation** beyond that bounded word-level repair; browser/platform APIs;
  randomness; wall-clock time.

## The invariants it guarantees

- **Deterministic:** identical words + constraints yield identical reports
  (byte-for-byte).
- **Pure in the words:** a constraint's verdict is a function of the word list
  alone.
- **Re-validatable repair:** a repaired word list, re-validated, satisfies every
  *repairable* constraint; the bound is explicit (one pass).
- **Stable, ordered scoring:** more satisfying words ⇒ a higher score.
