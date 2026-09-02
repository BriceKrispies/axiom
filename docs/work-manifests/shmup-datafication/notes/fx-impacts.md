# `fx/impacts.rs` — pilot slice

**Verdict: not converted. This file is 27 singleton algorithms sharing a
vocabulary, not 27 fillings of one shape.** Nothing was written to
`apps/axiom-shmup/src/fx/impacts.rs`. No `mod.rs` line is needed.

`ax shape` before: **1352 code lines**, 0.76 lit/line, 0.061 br/line, reuse 10.6,
vocab 41, verdict `mixed`. After: identical — the file is unchanged.

## What the measurement actually pointed at

`ax shape --vocab` reads `impacts.rs` as a table with a driver: high literal
density, `reset_spawn` × 27 (once per burst), `range` × 129. That is the
signature of *content*, and the content is real. What the metric cannot see is
whether the 27 bursts are **addressable by one row type**, and they are not.

The decisive measurement is the closed-vocabulary test applied to **assignment
right-hand sides** rather than to callees. Run over the three axes that carry a
burst's skeleton:

| axis | sites | distinct forms |
|---|---|---|
| `s.x` (origin) | 27 | **12** |
| `s.vy` (velocity assembly) | 24 | 12 (≈7 after folding `+c`) |
| `s.size1` | 28 | 6 |

Twelve distinct origin forms over twenty-seven candidate rows is ~0.44 distinct
forms per row. A table pays as that ratio goes to zero. Compare the two
in-repo controls:

- `audio/foley.rs:123` `IMPACT` — **12 rows, one struct, zero structural
  modes.** Every surface differs only parametrically (`f`, `q`, `g`, `decay`,
  `level`). Ratio 1 struct / 12 rows. `ax shape` verdict `data`.
- `fx/tracers.rs` `SPRITES` — 3 rows, 9 fields, all `Fixed`, one `bool` mode.
  Verdict `data`.
- `fx/impacts.rs` — 27 rows needing ~60 mode variants across 13 axes, almost
  every mode used by one or two rows. Verdict `mixed`, and `mixed` is right.

`docs/engine-datafication.md` §3 states the test outright: *"If you cannot name
the closed vocabulary a method selects over, it is an algorithm."* The vocabulary
here does not close.

## The axes, enumerated

Per-burst variation that a shared row would have to carry:

- **Direction basis** — `(v+n)*0.5` (concrete/wood/soft), `(v+n*1.3)*0.5`
  (plaster), `v*0.8+n*0.2` (metal), raw `n` (concrete wisp, metal smoke, ground,
  water drop), `inc*0.8-n*0.2` (glass), `inc*0.75-n*0.25` (flesh), raw reflect
  (foliage), *absent* (5 bursts have no cone at all).
- **Post-transform** — none / `toward_hemi` at eps 0.05, 0.1, 0.12 /
  `+ clamp_cone(COS55)`.
- **Cone spread/bias** — constant / band-dependent (concrete dust) /
  grazing-and-flier-dependent (metal spark) / back-flipped per particle (glass).
- **Speed** — absent / `Draw` / `Draw * e` / banded 3-way / flier-branched.
- **Origin** — 12 forms (above), including `p + disc_on + n*Draw`,
  `p + n*0.16 + v*0.07`, `p + n*0.012 + r*(0.5*e*0.3)`, `p - n*0.02`.
- **Velocity** — `v*sp`, `v*sp` with five different `+c` on y, `v*0.7+0.5`,
  `v*1.6`, `dvx*2.5 / Draw*e / dvz*2.5`, `vy = 0.7` alone.
- **Tile** — `Fixed` / `i%2` (both polarities) / `i%3` / `i%4` / `band==2` /
  `rng<0.4` / `rng<0.5`. **Two of these seven consume a draw and five do not.**
- **`size1`** — `Draw`, `Draw*e`, `Draw*e*band`, `=size0`, `=size0*0.4/0.6/0.9`,
  `Fixed*e`.
- **`rot`** — absent / `float*TWO_PI` / `signed*0.25` /
  `screen_angle(camera)+signed*0.2` (metal lobe only).
- **Colour** — 6 literals / literals × per-particle sun-dot `lit` (two different
  `lit` formulas) / `blackbody` pair / sand-tint with ×0.85 or ×0.8+×0.7 /
  rubber-branched / `r1 = r0*0.9`.
- **Emit** — `emit_add` / `emit_lit` / **`emit_lit` then a coin-gated `emit_add`
  from the same mutated descriptor** (glass shard).
- **Things that fit no row at all** — the recursive raycasting `spark`,
  `fx.haze`, `fx.blood_spatter_behind`, metal's `grazing > 0.6` decal fork,
  water column's dead `let _ = dvy` binding.

## The strongest candidate, costed exactly (not estimated)

`concrete`'s dust jet (`impacts.rs:314-345`) and `plaster`'s ejecta (`:546-577`)
are the tightest pair in the file: **identical draw sequence, identical field
set**, differing only in numbers. Source clearly wrote one by copying the other.

- Today: 32 + 32 = **64 lines**.
- As a table: `Jet` struct 11 + two rows at 10 + driver 30 + doc 4 = **65 lines**.

Break-even at N=2, plus a new indirection — which is `docs/engine-datafication.md`
§2 exactly: the saving is `(N−1) × per-variant-code` minus interpreter + format
overhead, and at N=2 that is zero.

Extending to the widest family I could make cohere — the nine "cone → speed →
debris" bursts (concrete chip, plaster chip, wood splinter, ground clod, glass
shard, foliage, soft splinter, flesh droplet, water droplet) — a row is ~17 lines
against a ~31-line burst, so 14 × 9 = 126 lines saved, against a struct + five
mode enums + driver at ~120–140. **Net ≈ 0 on 1352 lines, and it covers 9 of 27
bursts** — so the file would carry both the table machinery *and* the original
style. More concepts, not fewer: the junk drawer with a nicer name that
`CLAUDE.md` rejects.

## The file has already been datafied, by the porter

The two things in `impacts.rs` that genuinely repeat are already tables with
drivers:

- `spark` / `SparkOpts` — 3 call sites (`impacts.rs:44`).
- `bullet_hole` / `BulletHole` + `impl Default` — 5 call sites (`:148`).

That is the whole of the repetition. Everything else occurs once. The remaining
honest refactor is **ordinary function extraction** for the concrete↔plaster
pair — not datafication — worth ~30 lines on 1352, and it would fuse two recipes
an artist may well want to diverge.

## Draw sequence reconstructed: `wood`

`cone` consumes **2** draws (`util.rs:80,83`); `disc_on` consumes **2**
(`:204,205`); `toward_hemi` and `clamp_cone` consume **none**.

`wood(fx, point, n, inc, e)`, `q = fx.pscale`:

```
n_spl = (11*q).round() + 4
  per splinter:  cone ×2 | range(2.5,7.5) sp | range(0.014,0.045) size0
                 | range(0.6,1.2) life | float rot | signed spin | float seed
                 = 9 draws            (tile is i%4 — no draw)   -> emit_lit
n_dust = (5*q).round() + 2
  per dust:      cone ×2 | range(0.6,2.0) sp | range(0.05,0.12) off
                 | range(0.04,0.09) size0 | range(0.24,0.44) size1
                 | range(0.45,0.9) life | float rot | signed spin
                 | range(0.5,0.8) alpha | float seed
                 = 11 draws                                     -> emit_lit
bullet_hole:     range(0.05,0.075) size | float roll | float flip
                 | float sooty | range(0.18,0.30) halo_size
                 | range(0.08,0.15) opacity | float halo_roll
                 = 7 draws                                      -> 2 decals
total = 9*n_spl + 11*n_dust + 7
```

## Draw sequence reconstructed: `concrete`'s banded dust — the trap, quantified

`impacts.rs:257-312`, `band = i % 3`. The brief flags `:284` and it is worse than
it looks:

```
band 0:  cone ×2 | range(1.8,3.2) sp | disc_on ×2 | range(0.05,0.16) off
         | range(0.045,0.1) size0 | range(0.3,0.62) size1
         | delay = Fixed(0.0)  <-- NO DRAW
         | range(0.22,0.4) life | range(5.0,7.0) drag
         | float rot | signed spin | range(0.4,0.72) alpha | float seed
         = 13 draws
band 1:  ... | range(0.9,1.9) sp | ... | range(0.02,0.09) delay  <-- 1 draw
         | range(0.5,0.85) life | range(2.6,4.0) drag | ...
         = 14 draws
band 2:  ... | range(0.4,1.0) sp | ... | range(0.02,0.2)  delay  <-- 1 draw
         | range(1.1,1.8) life | range(2.6,4.0) drag | ...
         = 14 draws
```

**The three bands are three different-length draw sequences** (13/14/14), and
`n_dust = (9*q).round() + 3` is not in general a multiple of 3, so which bands
appear — and therefore the total draw count of the whole burst — depends on
`fx.pscale`. A band is a full row, as the brief says, and each row must carry
`Absent` for `delay`, never `Fixed(0.0)`, or band 0 takes a draw it never took
and every later effect in the frame shifts. The same shape recurs at
`plaster:489` with different bounds (`0.1` / `0.22`).

## A float trap the brief does not name

Folding `s.vy = vy2 * sp` and `s.vy = vy2 * sp + 0.45` into one slot with the
first row carrying `Fixed(0.0)` looks exact — `v + 0.0 == v` for every finite
`v`. It is not: `(-0.0) + 0.0 == +0.0`. Ten of the 24 `s.vy` sites are the bare
`vy2 * sp` form, and any of them can produce `-0.0`. The ledger digests IEEE
**bit patterns** (`03-capture-harness.md`: "`-0.0 ≠ +0.0`"), so this would fail —
correctly. The lesson generalises: the y-offset slot needs a real `Absent`, which
costs the driver the branch the table was supposed to remove.

## `ax` friction

Logged **`a933935`** (`--verdict tool`).

`ax shape`'s `reuse` column is the closed-vocabulary test applied to *callees*,
and on that axis `foley.rs` (14.0, a true 12-row table) and `impacts.rs` (10.6,
27 singletons) are not separable. The axis that decides table-vs-algorithm is the
closed-vocabulary test applied to **assignment right-hand-side forms**: for each
recurring left-hand side in a candidate block, how many distinct RHS shapes it
takes across the rows. I had to compute that by hand with three `ax q` regexes
over `s.x`, `s.vy` and `s.size1`.

Adding a `--rows` (or `--forms`) mode to `ax shape` that reports distinct-RHS-
forms per recurring LHS would let the orchestrator triage the remaining ~33
slices for a few seconds each, instead of a full agent read per file. On this
evidence that is the single highest-leverage change to the tool for this
programme.

## Recommendation for the fan-out

Do not dispatch an agent per file on the strength of `ax shape`'s literal density
alone. Density says *content is present*; it does not say *the content is
addressable*. Screen first on the RHS-form ratio above (by hand, or by fixing
`a933935`), and dispatch only where a candidate row type closes over its rows.
`foley.rs`-shaped slices — one struct, one row per enum variant, parametric
variation only — will convert. `impacts.rs`-shaped slices — one recipe per
variant, each with its own skeleton — will not, and an agent that forces one
will ship a stream shift that the whole-game witness catches late and localises
badly.
