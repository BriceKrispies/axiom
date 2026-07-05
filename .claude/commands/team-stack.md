---
description: Fan a task out to a team of agents — each works in its own git worktree and lands exactly ONE commit — then stack every commit onto a persistent CHAMPION worktree/branch off main (never main itself), and clean out the agent worktrees. Each invocation stacks another round onto the same champion line. No steering: every agent gets the whole task and its own angle.
argument-hint: <task description>  (optionally lead with a slug: `slug=<name> <task>`)
allowed-tools: Read, Grep, Glob, Bash, Agent
---

You are the **foreman** of a `team-stack` run. You take one task, hand it to a **team of
agents working in parallel**, and accumulate their work — one commit each — onto a
**persistent champion line**: a long-lived git branch/worktree that lives **off `main`** and
grows by one round every time this command is called. **You never touch `main`.**

**No steering.** You interpret the ask only far enough to (a) choose the team size and (b)
give each agent a short distinct *angle* label. You do **not** slice the task into
disjoint subtasks or tell agents which files to touch — every agent gets the **whole task**
and its own angle, and produces its own complete take. The value is the multi-angle read; keep
each agent's context minimal.

Arguments: `$ARGUMENTS` — the task in natural language. If it begins with `slug=<name> `,
use `<name>` as the champion slug; otherwise derive a short kebab-case `<slug>` from the task.

Note: no local commit hook in this repo (gates run in CI), so commits are fast. All commits in
this workflow use `--no-verify` because they are **proposals on a branch off `main`** — the CI
gates (`cargo xtask check-architecture`, coverage, dylint-gate, ts-gate) run only if/when the
champion line is later deliberately landed on `main` (a separate, human-approved step — never
part of this command).

## Step 1 — Interpret the ask & resolve the champion line (do this yourself)

1. **Read the task.** Decide the team size `N` (default **4**; scale to the task's breadth,
   cap at ~7) and assign each agent a one-word/short **angle** — a distinct perspective on the
   *same* whole task (e.g. `robustness`, `ergonomics`, `perf`, `minimal`, `tests-first`,
   `docs`, `structural`). Angles are labels only; they are not disjoint work slices.
2. **Champion worktree** (persistent, per slug): branch `team/champion-<slug>`, worktree
   `.claude/worktrees/team-champion-<slug>`. If it does **not** exist, create it off current
   `main`:
   ```sh
   git worktree add .claude/worktrees/team-champion-<slug> -b team/champion-<slug> main
   ```
   If it already exists, **reuse it** — this run stacks onto whatever rounds it already holds.
   (It lives off `main` on purpose: `main` may be churning with other work.)
3. `<base>` = the champion worktree's current tip:
   `git -C .claude/worktrees/team-champion-<slug> rev-parse HEAD`. Every agent rebases onto
   this so the round stacks cleanly on the accumulated line.

## Step 2 — Fan out the team IN PARALLEL, each in its OWN worktree

In a **single message**, spawn all `N` agents with the `Agent` tool, **each `isolation:
"worktree"`**, `subagent_type: "general-purpose"`. Give every agent ONLY this (fill in the
task, the angle, `<base>`, and a unique pinned branch `team/<angle>-<slug>`):

> You are one member of a parallel team, working the **`<angle>`** angle. The whole task is:
>
> «`<the full task, verbatim>`»
>
> You are in your own isolated git worktree. **FIRST run `git reset --hard <base>`** (rebase
> onto the champion line — do NOT skip this). Then do the task from your `<angle>` angle,
> end-to-end, and land **exactly ONE commit** on a pinned branch:
> `git switch -c team/<angle>-<slug>` (or `git branch -f` it to your commit), commit with
> `--no-verify`, and report the commit **sha**. Keep the change bounded and self-contained —
> one coherent commit, not a series. Do **NOT** touch `main`, do NOT merge, pull, push, or
> rebase onto anything but `<base>`. If you genuinely change nothing, report `Change: none`.

Wait for all `N`. Collect each `(angle, branch, sha)`. An agent may return `Change: none`.

## Step 3 — Stack every commit onto the champion branch

Cherry-pick each agent's commit onto the champion worktree, in the order the agents are
listed (stable, deterministic):
```sh
git -C .claude/worktrees/team-champion-<slug> cherry-pick <sha>
```
Because every agent worked the same task from a different angle, **cherry-pick conflicts are
expected**. On conflict, abort that one and record it **dropped (conflict)** — this is a
best-effort stack; keep going with the rest:
```sh
git -C .claude/worktrees/team-champion-<slug> cherry-pick --abort
```
The commits **stay** on the champion branch — this is the accumulating champion line, never
squashed, never moved to `main` here.

## Step 4 — Report to the human

Report factually:
- The team you formed (each angle) and, per agent, **stacked** / **dropped (conflict)** /
  **`Change: none`**, with a one-line summary of what each contributed.
- The champion line's new tip: `git -C .claude/worktrees/team-champion-<slug> log --oneline <base>..HEAD`.
- Any conflicts that were dropped and why — so the human can reconcile them by hand if the
  angle mattered.

Do not over-claim. If most agents conflicted and only one landed, say so plainly.

## Step 5 — Clean out the agent worktrees (keep the champion worktree)

Remove every per-agent worktree and its now-redundant branch (their commits already live on
the champion line as cherry-picks). **Keep** the champion worktree + branch.
```sh
git worktree list --porcelain | awk '/^worktree/{print $2}' | grep 'agent-' \
  | while read wt; do git worktree remove --force "$wt" 2>/dev/null; done
git worktree prune
git for-each-ref --format='%(refname:short)' refs/heads/team/ \
  | grep -v '^team/champion-' | while read b; do git branch -D "$b" 2>/dev/null; done
```
(Skip any worktree locked by another live session. Do NOT remove `team/champion-<slug>`.)

## Repeat / land

Each invocation stacks another round onto `team/champion-<slug>`. When the human wants it on
`main`: rebase/cherry-pick the champion line onto then-current `main`, run the four CI gates
for any spine change, and GPU-verify any `wgpu` change. That landing is a separate, explicit,
human-approved step — **never** part of this command.
