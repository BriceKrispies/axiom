# `axiom-proc-validate` — architecture

Makes generated artifacts **trustworthy**: validation verdicts, scoring, and
bounded repair over `proc` artifacts.

## What it is

- **`Constraint`** — a generic, domain-free check over an artifact's neutral
  words: `min_count(n)`, `max_value(v)`, `non_zero()`. A fieldless `ConstraintKind`
  + a threshold, dispatched through eval/repair tables (branchless).
- **`ProcValidateApi::validate(&Artifact, &[Constraint])`** — a deterministic
  [`ValidationReport`]: a `(kind_code, satisfied, score)` verdict per constraint,
  whether all passed, and the total score. Pure in the artifact's words.
- **`ProcValidateApi::repair(&Artifact, &[Constraint])`** — a single **bounded**
  pass of word-level fixes (clamp to a max, lift off zero) that returns a new,
  re-validatable `Artifact` (via `Artifact::from_words`).
- **`ValidationReport`** — serializable (canonical bytes) + a stable `StableHash`
  digest, so reports golden-compare independently of the artifact.

## Why it depends on kernel + proc

- **proc** — the `Artifact` it validates, and the repaired `Artifact` it builds.
- **kernel** — `StableHash` + `BinaryWriter` for the report's canonical bytes and
  digest.

## What does **not** belong here

- **No domain rules.** "Rivers must reach the sea", "a level is solvable", "a room
  has a door" are *domain module* concerns (Phase 9), consuming this layer's
  generic constraint vocabulary — never baked in here.
- **No unbounded repair.** Repair is one pass, not a loop-to-fixpoint, and it
  never invents content: a structural minimum-count failure is left unsatisfied by
  design (a documented, honest limit).
- **No generation** beyond that bounded word-level repair; browser/platform APIs;
  randomness; wall-clock time.

## The invariants it guarantees

- **Deterministic:** identical artifacts + constraints yield identical reports
  (byte-for-byte).
- **Pure in the words:** a constraint's verdict is a function of the artifact's
  words alone.
- **Re-validatable repair:** a repaired artifact, re-validated, satisfies every
  *repairable* constraint; the bound is explicit (one pass).
- **Stable, ordered scoring:** more satisfying words ⇒ a higher score.

## A note on the `proc` boundary

Phase 6 promoted `proc`'s artifact constructor to a public `Artifact::from_words`
(it was already how evaluation packaged its output). Repair needs to *produce* a
re-validatable artifact, and that is the artifact's own construction capability —
an artifact is just `(generator_version, words)`, so constructing one from words is
`proc`'s job to expose, not a forgery hatch. Determinism remains a property of
`ProcApi` evaluation, which `from_words` does not touch.
