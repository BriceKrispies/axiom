# Capture recipe — treasure-chest-pick

The champion for this target is a **real screenshot of the running app**, taken by the
app's own capture agent (`apps/casino-games/web/browser/agent_capture.py`, a Playwright
driver over the shell's `window.__casino` handle). There is no `axiom-shot` path — Casino
Games is a pure-TypeScript app on `@axiom/web-engine`.

**Reference:** the branded beach diorama (`reference.png`) — the treasure hunt dressed in
one white-label brand: "ACME" red across a top ribbon banner, a left pennant, a right
signboard, the sandcastle pennant, the crab's little flag, and a label on every chest. The
brand (name + color scheme) is **configurable** — it lives in the game's `gameSpecific.brand`
config (`presentation/branding/brand.ts`, `DEFAULT_BRAND` = the ACME livery) and is editable
in the SET UP panel ("Brand livery"). The lettering is real welded geometry (there are no
textures on `@axiom/web-engine`): `presentation/branding/glyphs.ts` is a 5×7 box-run font and
`label.ts` stamps it onto a surface through the same transform frame the surface rides — so a
chest's label squashes, grows, tilts and spirals **welded to the chest**, and long names
shrink uniformly to fit. The champion is captured with the default ACME brand, so it wears the
same livery as the reference.

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
session phase to reach `ready`, then waits for the frame to be **frozen** (the `frozen`
step polls the canvas until it stops changing — see below), and writes the canvas
**backing store** (exactly 960×600, no browser resampling).

## Why these parameters

- **`backend=canvas2d`** — the deterministic baseline backend; the legible flat render is
  the right comparison for the storybook diorama.
- **`seed=470573198`** — a pinned round seed so the champion always draws the same layout.
- **`shot=90`** — freezes the simulation at tick 90 (past the 24-tick intro, well inside
  `ready`) *and* pins the view clock, so the frame is a pure function of (seed, config,
  tick).
- **`frozen`** — the preset does **not** shoot at `phase:ready` (reached ~tick 24, while the
  idle chest dance and the palm sway are still animating — two captures there differ by ~1%
  of pixels on the moving edges). The `frozen` step blocks until the canvas backing store
  stops changing, which happens once the sim reaches the `shot=90` freeze. Only then is the
  frame settled. Verified with the `frozen` gate: two consecutive captures are
  **byte-identical**.
- **Framing** — the whole page's arcade chrome is excluded (`--clip native`). The
  reference's own chrome (the seed readout, REPLAY / SET UP, the mute button) is UI text
  and is **not** part of what this campaign converges.

## Other moments

`--scene chests-hover` (cursor resting on the centre chest) and `--scene chests-reveal`
(pick the centre chest, hold through the celebration) capture the later beats of the
ritual if a later pass targets them. Both run the sim live rather than frozen, so they are
deterministic only to the actions they issue.
