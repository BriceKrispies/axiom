# Capture recipe — treasure-chest-open (reveal close-up)

This campaign converges the **open-chest reveal close-up** — the hero framing of
Treasure Chest Pick after a chest has flown to the camera and its lid has opened,
the treasure hovering clear above it — toward a polished low-poly reference
(warm gold banding, a faceted glowing blue gem, a soft glow halo, a dark vignette).

It is a **different moment** from the `treasure-chest-pick` campaign, which targets
the branded 9-chest beach diorama in the `ready` state. This one is the single-chest
`revealing` beat.

Casino Games is a pure-TypeScript app on `@axiom/web-engine` — there is no
`axiom-shot` path. The champion is a real screenshot of the running app, taken by
the app's own capture agent (`apps/casino-games/web/browser/agent_capture.py`, a
Playwright driver over the shell's `window.__casino` handle + boot URL).

## Reproduce the champion

```sh
# 1. serve the app (the champion worktree, when re-rendering a convergence round)
uv run scripts/localhost_servers.py start-app casino-games --port 8087

# 2. capture — the centre chest OPENED, lid up and the treasure showing, BEFORE the points
#    banner, frozen deterministically at tick 180
uv run apps/casino-games/web/browser/agent_capture.py \
    --scene chests-opened \
    --url http://localhost:8087/ \
    --out apps/casino-games/visual_targets/treasure-chest-open/candidate.png
```

The `chests-opened` preset expands to the boot URL
`?game=treasure-chest-pick&seed=470573198&shot=180&backend=canvas2d&press=Space@30`
and drives the sim as follows:

- **`press=Space@30`** — scripts the pick: a boot-time key press at tick 30 selects
  the default-focused centre chest (choice index 4), so no live pointer is needed.
- **`shot=180`** — freezes the simulation at tick 180 *and* pins the view clock. The
  reveal timeline (speed 1, reduced motion off) opens the lid at reveal-age 64
  (tick ~160) and only hands off to `celebrating` — when the "5 points" banner is
  drawn — at age 110 (tick ~206). Tick 180 sits squarely in the post-lid, pre-points
  window: the chest is open, the treasure is up, no score text is on screen.
- **`frozen`** — the preset blocks until the canvas backing store stops changing
  (the sim has reached the freeze tick), so the capture is a pure function of
  (seed, config, tick): byte-identical on every run.
- **`backend=canvas2d`** — the deterministic baseline backend; `--clip native` writes
  the 960×600 backing store with no browser resampling and no arcade chrome.

Note: as of the prize-catalog change the chest no longer yields a rarity gem. It
yields one of five modelled treasures — a gold bar, a gold coin, a diamond ring,
an old boot, or the beach crab's girlfriend — chosen by the reward tier the round
committed, so WHICH object appears is a function of the seed. Seed 470573198
yields the diamond ring; 11 the gold bar, 7 the gold coin, 22 the old boot, 55 the
crab. Pin the seed when comparing frames, or you are comparing two different
objects.

## Reference

`reference.png` — a polished low-poly treasure chest, lid fully open and hinged back,
a faceted blue gem hovering in the mouth inside a soft radial glow, warm gold corner
banding and latch, rich wood, a dark vignette with faint beach props in the corners.
An external/aspirational target (not an Axiom render).
