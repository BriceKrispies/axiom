---
description: One convergence pass. Fans out the 7 lens agents — each in its own git worktree rebased onto the persistent CHAMPION worktree (never main) — with NO steering (just the reference + the current champion + "from your lens, make the commit"). Stacks every commit onto the champion branch, renders it, promotes the render as the new champion, shows a reference|before|after comparison, and cleans out the agent worktrees. Repeatable: each call stacks another round onto the same champion line.
argument-hint: <target-dir> [app/game name]
allowed-tools: Read, Grep, Glob, Bash, Agent
---

You are the **foreman** of the visual-convergence board. The board converges a **champion
line** — a persistent git branch/worktree that accumulates every round's commits — toward a
reference, entirely off `main`. One invocation = one pass. **You never touch `main`.**

The lenses get **no direction from you.** You do not tell them which axis to attack, which
file to touch, or what's wrong — you hand each one only *the reference, the current
champion, and its own lens*. Each agent reads the two images, scores from its perspective,
picks its own flaw, and commits its own fix per its definition. Steering the lenses defeats
the point (the multi-angle read is the value); keep the context minimal.

Arguments: `$ARGUMENTS` — first token = **target directory** (`reference.png`,
`champion.png`); optional second = **app/game name**.

Note: no local commit hook in this repo (gates run in CI), so commits are fast; proposal and
stack commits use `--no-verify` because they are proposals — the CI gates run only if the
champion line is later landed on `main`.

## Step 1 — Resolve the pass (do this yourself)

1. **Champion worktree** (persistent, per target): branch `convergence/champion-<slug>`,
   worktree `.claude/worktrees/convergence-champion-<slug>` (`<slug>` = basename of the
   target dir). If it does **not** exist yet, create it off current `main`:
   ```sh
   git worktree add .claude/worktrees/convergence-champion-<slug> -b convergence/champion-<slug> main
   ```
   If it already exists, **reuse it** — this pass stacks onto whatever rounds it already
   holds. (It lives off `main` on purpose: `main` may be churning with other work.)
2. `<base>` = the champion worktree's current tip: `git -C <champion-wt> rev-parse HEAD`.
   Every agent rebases onto this.
3. **Champion image** the lenses score against: the champion worktree's
   `visual_targets/<slug>/champion.png` (first pass: seed it from `<target-dir>/champion.png`
   if the champion worktree doesn't have one). Absolute path — worktrees only contain
   tracked files, and agents read images by absolute path. **Reference**:
   `<target-dir>/reference.png` (absolute). Confirm both exist; if not, **ask**.
4. **App/game name** + **source dir** (`apps/axiom-gallery/src/<name>/`, `apps/axiom-<name>/`,
   `games/<name>/`). If unknown, **ask**.
5. **Capture recipe** for the render (from the scorecard/manifest or the skill's Step 1):
   `--app`, `--backend`, `--tick`, any `--pose`/`--script`. Default `--backend gpu --tick 0`.
   (Some harnesses need `--features offscreen` — check the app's `axiom-shot` wiring.)

## Step 2 — Fan out the 7 lenses IN PARALLEL, each in its OWN worktree — NO steering

In a **single message**, spawn all seven with the `Agent` tool, **each `isolation:
"worktree"`**. Give every agent ONLY this — no axis, no findings, no "attack X", no file hints:

> You are the `<lens>` lens of the visual-convergence proposal board. Read
> `C:\dev\axiom\.claude\agents\convergence-<lens>.md` and follow it.
> Reference image (absolute): `<abs>/reference.png`
> Champion image (absolute): `<abs-champion-png>`
> App/game: `<name>` · Source to edit in YOUR worktree: `<source-dir>`
> Base: `<base>`. **FIRST run `git reset --hard <base>`** (rebase onto the champion line — do
> NOT skip). Then, from your lens alone, score the champion against the reference, pick the
> flaw *you* see, and make ONE bounded commit per your definition: `--no-verify`, pinned
> branch `convergence/<lens>-<slug>`. Read the images by absolute path; do NOT build/render.
> Do NOT touch main / merge / pull / push.

The seven `subagent_type`s: `convergence-art-director`, `convergence-modeler`,
`convergence-lighting`, `convergence-surfacing`, `convergence-colorist`,
`convergence-rigger` (skeletal-rigging / character-pose specialist — commits a bounded
pose change), `convergence-critic` (bookkeeping to the target dir). Wait for all seven;
collect each `(lens, branch, sha)`. A lens may return `Change: none`.

(Roster is swappable per target: `convergence-engine-architect` — spine-feasibility /
lowest-correct-layer, advisory-or-commit — is available and belongs in the roster whenever
a target's gaps are structural/spine rather than character-pose. Swap it in for the rigger,
or run 8 lenses, as the target warrants.)

## Step 3 — Stack every commit onto the champion branch (keep them there)

Cherry-pick each proposal commit onto the champion worktree, in a stable order (app lenses,
then any spine commit, then critic bookkeeping last):
```sh
git -C .claude/worktrees/convergence-champion-<slug> cherry-pick <sha>
```
On conflict, abort that one (`cherry-pick --abort`) and record it **dropped (conflict)** —
best-effort stack, keep going. The commits **stay** on the champion branch; this is the
accumulating champion line, never squashed, never moved to `main` here.

## Step 4 — Render the champion + promote

From the **champion worktree** (isolated `target/`), render with the Step-1 recipe:
```sh
cargo run --manifest-path .claude/worktrees/convergence-champion-<slug>/tools/axiom-shot/Cargo.toml \
  --release [--features offscreen] -- --app <name> --backend <backend> --tick <N> \
  --out <scratch>/candidate.png
```
Then **promote** it as the new champion on the line:
```sh
cp <scratch>/candidate.png .claude/worktrees/convergence-champion-<slug>/visual_targets/<slug>/champion.png
git -C .claude/worktrees/convergence-champion-<slug> add visual_targets/<slug>/champion.png
git -C .claude/worktrees/convergence-champion-<slug> commit --no-verify -m "champion: promote pass render"
```
(So the next pass automatically scores against this render.)

## Step 5 — Show the human

Composite **reference | before (prior champion) | after (this pass)** (PIL: equal height +
label bars) and `SendUserFile` it with `display: "render"`. Report: which lenses committed
vs `Change: none`/dropped; the architect's advisory (any spine commit + gates a real landing
needs); the critic's re-score + next-attack axis. Keep it factual — no over-claiming parity.

## Step 6 — Clean out the agent worktrees (keep the champion worktree)

Remove every per-lens agent worktree and its now-redundant branch (their commits live on the
champion line). **Keep** the champion worktree + branch.
```sh
git worktree list --porcelain | awk '/^worktree/{print $2}' | grep 'agent-' \
  | while read wt; do git worktree remove --force "$wt" 2>/dev/null; done
git worktree prune
git for-each-ref --format='%(refname:short)' refs/heads/convergence/ \
  | grep -v '^convergence/champion-' | while read b; do git branch -D "$b" 2>/dev/null; done
```
(Skip any worktree that's locked by another live session.)

## Repeat / land

Each invocation stacks another round onto `convergence/champion-<slug>`. When the human wants
it on `main`: rebase/cherry-pick the champion line onto then-current `main`, and for any spine
commit run the four CI gates (`cargo xtask check-architecture`, coverage, dylint-gate,
ts-gate) + GPU-verify wgpu changes. That landing is a separate, explicit human-approved step —
never part of this command.
