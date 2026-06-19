# Adversarial review plan — Growth app vs the requirements audit

Purpose: continuously prove that **everything in `worldgen_simulator_requirements_audit.md` is genuinely accounted for** in `apps/axiom-growth`, and surface — adversarially — anything missing, faked, or under-built. The builders are optimistic; these reviewers are paid to be hostile.

The single source of truth for "what must exist" is
`docs/growth-port/worldgen_simulator_requirements_audit.md` (its **trace matrix** especially). The in-code mirror is `apps/axiom-growth/src/requirements.rs` (the [`requirements::REQUIREMENTS`] registry). Reviews cross-check the three corners: **audit ↔ registry ↔ actual code**.

## The reviewer roster

Run these as independent agents. Each writes a findings file under
`docs/growth-port/reviews/<reviewer>-<date>.md` with a verdict table and a
prioritized gap list. They must **not** edit engine/app code — they only read,
run tests, and report. Spawn them after each build wave.

### R1 — Coverage auditor ("is every requirement present?")
- Walk every row of the audit trace matrix. For each, find the matching entry in `requirements.rs` and the named `site` in code. Flag: requirements in the audit with **no registry entry**; registry entries whose `site` module/function **does not exist**; audit categories with thin coverage.
- Output: a table `requirement → in registry? → site exists? → status` and a list of unaccounted requirements.

### R2 — Honesty / stub auditor ("is 'Implemented' actually implemented?")
- For every requirement marked `Implemented` in the registry, open its `site` and judge whether the body does real work or is a stub / no-op / `todo!` / returns a constant / `STUB` comment. Grep the tree for `STUB`, `TODO`, `unimplemented`, `todo!`, no-op `run()` bodies.
- Flag any `Implemented` that is actually a placeholder (status inflation). Recommend the correct status (`Scaffolded`/`Deferred`).

### R3 — Fidelity auditor ("does it meet the acceptance criteria?")
- For each implemented subsystem, read the audit's **acceptance criteria** for the matching story and check the code meets the *semantics*, not just the name. Examples: does `moisture_advection` actually move moisture downwind, or just copy? does `fit_land_coverage` hit the target within tolerance? does `locate_region` with the index agree with brute force? is generation **bit-deterministic** across runs? does the chunk seam delta = 0?
- Run `cargo test -p axiom-growth` and read the tests: flag subsystems whose tests are weak (assert nothing meaningful) or absent.

### R4 — Determinism / QA gate auditor (optional, periodic)
- Specifically attack determinism and the audit's QA gates (SC-E1 ring validity, SC-E3 determinism hash, SC-E8 seam/adjacency, OW-E21 land-fraction tolerance). Construct inputs that try to break them. Flag any gate that is asserted but not actually enforced by a test.

## How a review cycle runs
1. Builders land a wave; `cargo test -p axiom-growth` is green and `cargo xtask check-architecture` passes.
2. Spawn R1–R3 (and R4 periodically) in parallel; each writes its findings file.
3. Triage findings into: **missing** (no code), **inflated** (status wrong), **shallow** (real but below acceptance criteria). Update `requirements.rs` statuses honestly and open the next build wave against the gap list.
4. Repeat until R1 shows full coverage and R2/R3 show no inflation/shallowness.

## Done definition (audit fully accounted for)
- R1: every audit trace-matrix row maps to a registry entry with an existing site.
- R2: no `Implemented` is a stub; statuses are honest.
- R3: every `Implemented` subsystem meets its audit acceptance criteria with a meaningful test.
- Anything genuinely later-phase is `Deferred`/`Scaffolded` **on purpose**, traceable, and listed — not silently dropped.
