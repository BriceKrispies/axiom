# Capture recipe — treasure-chest-pick

The champion for this target is a **real screenshot of the running app**, taken by the
app's own capture agent (`apps/casino-games/web/browser/agent_capture.py`, a Playwright
driver over the shell's `window.__casino` handle). There is no `axiom-shot` path — Casino
Games is a pure-TypeScript app on `@axiom/web-engine`.

## Reproduce the champion

```sh
# 1. serve the app (the champion worktree, when re-rendering a convergence round)
uv run scripts/localhost_servers.py start-app casino-games --port 8087

# 2. capture — nine chests showing, canvas2d, frozen at tick 90 on the reference's seed
uv run apps/casino-games/web/browser/agent_capture.py \
    --scene chests-ready \
    --url http://localhost:8087/ \
    --out apps/casino-games/visual_targets/treasure-chest-pick/candidate.png
```

The preset expands to the boot URL
`?game=treasure-chest-pick&seed=470573198&shot=90&backend=canvas2d`, waits for the
session phase to reach `ready` (never a wall-clock sleep), and writes the canvas
**backing store** (exactly 960×600, no browser resampling).

## Why these parameters

- **`backend=canvas2d`** — the deterministic baseline backend; the reference is a flat,
  unlit-looking storybook frame, so the legible flat render is the right comparison.
- **`seed=470573198`** — the seed the reference frame carries in its own HUD readout
  ("seed 470573198 · round 1"), so the champion draws the same round the reference shows.
- **`shot=90`** — freezes the simulation at tick 90 (past the 24-tick intro, well inside
  `ready`) *and* pins the view clock, so the frame is a pure function of (seed, config,
  tick). Verified: two consecutive captures are **byte-identical**.
- **Framing** — the whole page's arcade chrome is excluded (`--clip native`). The
  reference's own chrome (the seed readout, REPLAY / SET UP, the mute button) is UI text
  and is **not** part of what this campaign converges.

## Other moments

`--scene chests-hover` (cursor resting on the centre chest) and `--scene chests-reveal`
(pick the centre chest, hold through the celebration) capture the later beats of the
ritual if a later pass targets them. Both run the sim live rather than frozen, so they are
deterministic only to the actions they issue.
