# Claude of Duty (`apps/shmup`) — agent routing

A self-serving app: it vendors its own toolchain (Vite, three.js) and is served
by `axiom-serve` handing it the port. It is not part of the engine dependency
graph, so the Layer/Module/Branchless/Coverage laws in the repo-root
`CLAUDE.md` do not bind the code here — but the No-Shortcuts rule does.

Read `README.md` for the tool inventory and `ARCHITECTURE.md` for the
subsystem map.

## The number that matters is SETTLED

**Settled is the moment the picture stops changing by itself — the point after
which nothing moves unless the player moves it. That is the headline figure for
this app, and every other boot number is a diagnostic that explains it.**

Operationally: the later of

* the last thing to arrive — the final texture painted, the last program linked,
  the last streamed object added; and
* the end of the last frame stall over ~200 ms.

Take whichever comes second. A game that has finished loading but still hitches
has not settled, and neither has one that runs smoothly while its walls are
still swapping materials underneath the player.

The reason it is the headline and not `first-frame` is that **a player cannot
tell "still loading" from "loaded badly".** Everything before settled is the
game announcing that it is not ready yet, and it announces this in whatever
vocabulary it has to hand: flat stand-in materials where the lighting should be,
empty hands where the weapon should be, an untextured wall, a five-second
freeze. The player reads all of that as one thing — *not ready* — and the clock
they are running started at navigation.

So a boot that hands over control in 2 s and then spends 40 s visibly assembling
itself is a 40-second boot, and reporting it as a 2-second boot is not an
optimistic reading, it is the wrong measurement. **Optimise the ladder, not its
first rung.** Moving `first-frame` earlier while pushing work behind it moves
nothing; it relocates the wait to somewhere the instrument was not looking.

The rest of the ladder, worst first — read these to find out *why* settled is
where it is, never as goals in themselves:

| Milestone | What it means |
|---|---|
| `loaded` | Every stream generator drained and the final pre-warm finished. The best single proxy for settled that the profiler prints on its own. |
| weapon in hand | The player has a gun. Before this they can walk, and that is all. |
| `lit` | The fidelity ramp released — the level stops being flat-lit stand-ins. |
| `first-frame` / `input-live` | The first painted frame, and the first frame that polls input. The most flattering number in the app and the least meaningful alone. |

**Frame stalls after the first frame are boot cost.** They are also invisible to
`bootprofile`, which closes its span tree at `__READY__` — and on this app
almost everything interesting happens after that. Sample them with a
`requestAnimationFrame` delta loop and treat any gap over ~200 ms as part of the
boot you are trying to shorten.

That loop has to run **inside a cold run**, injected into the `--icy` page. It is
tempting to sample stalls through the Playwright controller because it is quick
— that is a warm browser on a warm driver, and stalls are precisely the thing
the driver's shader cache removes. A stall profile taken warm is a profile of
the one machine that does not have the problem.

## Measure COLD. It is the only measurement.

**There is one boot measurement for this app and it is the cold one. Do not run
warm timings at all — not as a quick check, not as a sanity pass, not "just to
see the direction". A warm run is not a cheaper approximation of a cold run; it
is a different experiment, and its answer is routinely the opposite one.**

```sh
node tools/bootprofile.mjs --icy --input --no-glprobe --repeat=3
```

`--icy` empties the GPU driver's on-disk shader cache before the run. Without
it you are measuring a machine that has already compiled this app's shaders,
which is every machine except a player's.

Why this is a rule and not a preference:

* **The driver's program cache is per MACHINE, not per browser profile**, and it
  is keyed on shader source. Clearing browser data does nothing. The person most
  likely to measure this app is the person who just ran it, and their driver is
  the one driver in the world that is already warm for it.
* **The gap is not a detail, it is the whole subject.** This app's cold boot is
  dominated by serial GPU shader compilation — roughly 23 s of driver work
  across ~110 programs on a first visit, versus near-zero warm. A warm run does
  not measure a faster version of the same thing; it measures a different thing
  with the expensive part deleted.
* **It has produced three wrong conclusions here, in one session.**
  * A fix for a bug that doubled every texture-bake shader compile: warm, it
    restored `loaded` to its old figure exactly and read as "regression closed".
    Cold, it had barely moved — 59.8 s against a 44.5 s baseline.
  * Letting the streamer step over a generator waiting on the GPU: warm, the
    weapon reached the player's hands at 3.0 s instead of 30.0 s and it looked
    like the best change of the day. Cold, it pushed `__READY__` from 2.8 s to
    16.3 s and `loaded` from 59.8 s to 67 s — because a cold driver has one
    serial compiler and letting everything stream at once means nothing
    finishes. **A 27-second win became a regression on every milestone.**
  * Baking the surface textures at build time: warm, worth ~14 s. Cold, this
    was measured three times and reported three different ways, which is the
    lesson. Neutral (67.6 s vs a 67.1 s control); then a 24.5 s win once the
    control was re-taken; then neutral again — **settled 34.9 s baked against
    34.8 s procedural** — once a 27 s regression was removed from the
    PROCEDURAL side, which had been inflating the comparison. Measuring cold is
    necessary and not sufficient: hold everything else still, or a cold number
    will mislead you exactly as thoroughly as a warm one.

Corollaries:

* `--repeat=3` and quote the median. A single run on this machine varies by more
  than most changes are worth.
* **There is no labelled-warm escape hatch.** A warm figure with a disclaimer is
  still a warm figure, and the disclaimer will be dropped by whoever quotes it
  next — including you, two messages later. If you cannot run it cold, you do
  not have a number yet.
* **The Playwright controller and a plain browser reload are warm by
  construction.** They are for looking at the picture — did it render, is the
  colour right, did the level light up — and never for timing. Every number in
  this app comes from `bootprofile.mjs --icy`.
* `--icy` is best-effort: it cannot delete a cache file the GPU process still
  holds open. The report classifies each run from its own shader counters and
  says `!! THIS RUN WAS NOT COLD` when the delete did not take. Believe the
  classifier, not the flag.
* Cold runs are slow and there will always be a reason to skip one. That
  pressure is exactly why this is a rule: the warm run is fast, it is available,
  it is directionally plausible, and on this app it has been wrong every single
  time it disagreed with the cold one.

### Measuring cold is not the same as only caring about cold

Two different questions, and conflating them is its own mistake:

* **"Did this work?"** is answered cold, always, by the rule above. Cold is the
  only regime in which the expensive part of this app is present, so it is the
  only regime in which a change can be shown to have done anything.
* **"Is this worth keeping?"** is a wider question. A first visit is cold; every
  visit after it is warm, and for a returning player the warm path is the one
  they actually live in. **A change that is cold-neutral and warm-positive is a
  real improvement and should be kept.**

So the gate is: measure cold, require that cold does not regress, and then a
warm gain is a legitimate reason to ship. What is forbidden is the reverse —
letting a warm gain stand in for a cold measurement that was never taken.
Warm may be measured *in addition*, never *instead*, and never before cold has
cleared the change.

The pre-baked surface textures (`tools/bake-textures.mjs`) are the worked
example: cold they are neutral — settled 34.9 s baked against 34.8 s
procedural, medians of three — and warm they take `loaded` down by ~14 s.
Cold-neutral, warm-positive, so they stay. Quoting the 14 s as a boot
improvement would have been the lie.

One caveat that applies to every "cold-neutral" claim made on this machine:
`bootprofile` serves from localhost, so a download costs nothing in the
measurement and a great deal on a real connection. Asset size is a cost the
cold number cannot see. Weigh it separately.

## Fidelity: `lean` by default, `full` on request

`core/fidelity.js` is the source of truth. **A different axis from `quality`**,
and confusing the two wastes time:

* **quality** (`?q=low|medium|high|ultra`) scales things that cost FRAME time —
  render scale, shadow map size, cascades, particle budgets.
* **fidelity** (`?fidelity=lean|full`) scales things that cost COLD BOOT time —
  how much shader text exists, and so how long the driver spends compiling it on
  a first visit.

They are independent because the costs are independent. Measured: dropping
`ultra` to `low` barely moved the cold boot and compiled MORE programs, because
the presets never touch the material shaders, which is where the text is.

**`lean` is a programs budget.** The governing arithmetic is linear and dull:

```
cold boot  ~=  (number of lit programs)  x  (~100 KB of translated HLSL each)
```

The per-program factor is immovable — fewer lights bought 4%, fewer cascades
13%, and swapping the material class to Lambert rendered nothing, because the
bulk of a program is three's shared lighting and shadow plumbing rather than
anything this app picked. **The program COUNT is the only lever that has ever
worked**, and it works reliably: 101 -> 83 measured -16%, 83 -> 60 measured
-18%, 60 -> 53 measured -15%.

| cold, settled | programs | |
|---|---|---|
| `full` | 101 | ~26 000 ms |
| **`lean` (default)** | **53** | **15 583 ms** |

What `lean` gives up, each of them a deliberate decision: surface ornament
(parallax, weathering, patch/cloth/detile/macro); surface variety (19 library
surfaces folded onto 4); AI character variants; the whole post chain (SSR, GTAO,
contact shadows, TAA, motion blur, ADS DOF, bloom, FXAA); the sky (dome,
volumetrics, LUTs, IBL — nine programs, replaced by a flat colour); and fx
(particles, tracers, muzzle flash, impacts, decals, shells, explosions).

**The weapon is deliberately not on that list.** The gun in your hands is this
game's identity in a way the sparks around it are not — that is an art call, and
it outranks the arithmetic.

Three things to know before changing it:

* **Projection is not ornament.** The first attempt dropped `OW_TRIPLANAR` with
  everything else; the ground and road lost their projection and the level
  rendered grey. It measured 6 s faster than the correct version, which is how
  a broken picture gets mistaken for a trade. Look at the frame, not just the
  number.
* **A flat background colour is not free of plumbing.** The renderer holds
  `autoClearColor = false` because it owns its own clears. three sees
  `scene.background.isColor`, sets the clear colour and raises its own
  forceClear — but the clear it then issues passes `autoClearColor` and so
  paints nothing. The sky came out black with the colour correctly set on the
  renderer. `render/index.js` clears to it by hand in the forward pass.
* **Canonicalise VALUES, not cache keys.** `onBeforeCompile` runs once per
  PROGRAM, not per material, so two materials collapse into one program only if
  they agree on the numbers they feed it.

**Capture mode forces `full`**, because the pixel gate's references are of the
full renderer. That is a deferred decision, not a settled one: the gate now
verifies a path players do not get by default. Either re-baseline the references
against `lean`, or keep gating `full` and accept that `lean` is unguarded.

## Progressive boot

`config.progressiveBoot` (set from `?ramp`/`?prewarm`, off in capture mode) puts
the game on screen before it is finished. Each subsystem reads it in its own
`init()` and holds back what the first frame does not need — the render system
keeps the post chain and cascades out of the frame, the sky holds its IBL and LUT
bakes, materials hold their surface bakes. `src/main.js` releases them in
priority order once there is a first frame to release them behind.

`?hold-post=0`, `?hold-sky=0`, `?hold-bakes=0` disable the three holds
individually — that is how you bisect "the progressive path renders wrong",
which is otherwise one symptom with three suspects. `?bakes=skip` drops the
surface bakes entirely (measurement only; the level renders untextured).

**The rule that governs all of it: hand the driver the work, wait for it, then
draw.** Drawing with a program the driver has not finished blocks the main
thread until its entire queue drains, and the cost is charged to whichever
program happened to ask — which is why a stall shows up on an innocent shader
and the real cause is somewhere else entirely. `warmFullScreen`/`materialReady`
(`src/render/pass.js`), `warmBlit` (`src/sky/fullscreen.js`) and
`TextureForge.issueProgram` (`src/materials/generator.js`) all exist for this.

Warming has one trap, and it has been fallen into: **three folds the bound
render target's colour space into the program cache key.** Warm with the canvas
bound and you link a program nothing will ever draw, the readiness check passes
for it, and the real program still compiles synchronously inside the first draw.
Always warm against the same target the draw will use.

## Capture mode is the pixel gate, and it opts out of all of this

`?capture=1` sets `deterministic`, disables progressive boot, drains every
stream and awaits pre-warm before raising `__READY__`. That is what keeps
`tools/imagediff.mjs` meaningful. Nothing in the progressive path may change what
capture mode renders — if a change needs a capture re-baseline, that is a
decision to raise, not to make.
