# Champion archive — treasure-chest-pick

`champion.png` is **overwritten by every pass**, so a landed champion that exists only
in git history is progress nobody can look at. This directory is the durable record:
one PNG per *landed* champion, never touched again once written.

Only the **judged arm** is archived. Guard images (`canvas2d-guard.png`) are per-pass
diagnostics, not history, and are deliberately not kept here.

A score is only comparable to the reference it was measured against, so every row names
its **reference era**. Eras for this target (see `campaign.toml`):

| era | reference | note |
|---|---|---|
| A | `4921ba29` | the original reference |
| B | `2e70a614` | |
| C | `f233fe43` | 1614×974, aspect 1.657 |
| D | `1e821742` | 1536×1024, aspect 1.50 — current, installed 2026-08-06 |

Scores are additionally only comparable **from pass 6 onward**, when the judged arm
moved from `canvas2d` to `webgl2` (`comparable_from_pass = 6`).

## Landed champions

| # | date | commit | era | pass | lenses landed | final | lowest axis after |
|---|---|---|---|---|---|---|---|
| 0000 | 2026-08-06 | `ea7e4724` | D | 8 | art-director, modeler, lighting, colorist, surfacing, critic | 1.25 | `material_and_texture_detail` |

### 0000 — pass 8

The first archived champion, and the archive's own starting point: **this index begins
at pass 8, not pass 1.** Eleven champion images were promoted and landed before it, none
archived — that gap is real and is recorded here rather than papered over. Earlier
champions are recoverable from git history (`git log --follow -- champion.png`) if a
later pass wants to backfill them; they are absent, not lost.

Six of eight lenses landed. The engine-architect returned an advisory with no commit,
having found every spine lever the gap needs already shipped at the correct layer. The
rigger's crab-pose proposal was **stacked, rendered, and then rejected by the human on
sight** and reverted before landing (ledger pass 8, `kind = "human_rejection"`) — the
board proposing and a human declining is the mechanism working, not a failure of it.

The pass also carried a **human arm constraint**: changes were to land on the judged
`webgl2` arm without adding cost to the `canvas2d` guard. Held — all three
geometry-adding proposals gated on `rendererTierAtLeast("webgl2")`, guard node count
byte-identical at 398/410.
