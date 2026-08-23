# The dev console — reading a symbol off a screenshot

`apps/shmup/src/scene/console.rs`. Turn it on from anywhere that can run JS:

```sh
uv run scripts/playwright_controller.py eval "window.__ax_console('ids on')"
uv run scripts/playwright_controller.py screenshot named
```

Every label on the picture is the **palette key** — the exact string
`scene::install` keys its material lookup off — so the label *is* the search
term:

```sh
target/release/ax q burlap     # -> apps/shmup/src/materials/mod.rs:737
```

## Commands

| command | answers |
|---|---|
| `ids on` \| `ids off` | the overlay, and how many entities are tagged |
| `ids` | current state, tag count, radius |
| `radius <m>` | how far to label (default 40) |
| `find <text>` | which tag names contain `<text>` |
| `names` | every distinct tag name in the level (44 today) |

## Why it exists

This port lost more time to *"the flat white thing beside the sandbags"* than to
any actual defect. A screenshot described a shape; the codebase held a symbol;
nothing connected them, so every visual bug began with a round of guessing which
material was on screen. The console removes the guessing step.

It is a console rather than a build flag on purpose: a flag is set before the
build, so noticing something mid-session costs a full wasm rebuild — and a
rebuild is exactly when a wasm-only break slips past unnoticed.

## What it is built on

[`axiom_introspect::WorldTag`] — the engine's own semantic noun (stable name,
coarse kind, world position). It already existed and had **zero consumers**; this
is its first. If another app wants the same affordance, the tag type is the part
to reuse, and only the projection and the command table are app-tier.

## The one non-obvious rule

`labels()` **declusters**: one label per ~104×22 px cell, nearest wins, capped at
60. Every *placement* is tagged (8,164 of them), which is what lets a label point
at one crate rather than at a category — but undeclustered that paints 747 labels
on a 1280×720 view, a green smear strictly worse than no overlay because it also
hides the thing it names. The first working build did exactly that. Nearest-wins
with a name tiebreak also makes the overlay **stable**, so two screenshots of the
same view are comparable.
