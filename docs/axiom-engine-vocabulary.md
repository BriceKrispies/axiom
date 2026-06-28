# Axiom Engine Vocabulary — mapped from the 11 hosted games

> Status reflects the Axiom tree at the time of writing. `have` = exists in the engine vocabulary
> today; `partial` = a related capability exists but not in the form a game needs; `missing` = no
> equivalent. ⚠ marks a **determinism hazard** — a capability whose current implementation breaks
> a deterministic, replayable, authoritative engine.

## The games

| Abbr | Game | What it is |
|------|------|------------|
| TF | tile-flipper | memory match · **DOM/CSS** |
| BC | block-cascade | match-3 · Canvas2D |
| BB | bonus-blitz | whack-a-mole · **DOM/CSS** |
| LL | last-light | turn-based roguelike · **DOM text-grid** |
| RS | reflex-sequence-master | Simon-says · DOM + Web Audio synth |
| NC | neon-clash | realtime fighter · Canvas + WebSocket — **already server-authoritative** |
| NI | neon-invaders | shoot-'em-up · Canvas2D |
| RH | rhythm-hero | rhythm · Canvas + **live-audio FFT** |
| 1v1 | one-on-one | arcade basketball · Canvas + PNG sprites |
| PM | pointman | pac-man · Canvas + seeded maze |
| NR | nova-roll | 3D marble · **CPU software rasterizer + cannon-es physics** |

---

## The three truths the mapping makes unavoidable

1. **The gap is 2D, not 3D.** Axiom is a 3D scene-graph engine and natively covers nova-roll (the one
   true 3D game). But **9 of 11 games are 2D** — sprites, shapes, text, gradients, particles, tilemaps —
   and Axiom exposes *no* 2D authoring surface today. Five games are pure DOM/CSS, drawing with `<div>`s
   or `fillRect`. The single largest body of missing vocabulary is a 2D surface.

2. **Determinism is the migration tax.** Only neon-clash is authority-ready (it already runs server-side
   on `DataDrivenGame`). The other ten lean on unseeded `Math.random`, wall-clock `setTimeout`/`Date.now`,
   and variable-dt loops — all of which must move to a seeded RNG + fixed-step engine clock.
   **pointman and nova-roll already seed their procgen** (Mulberry32), so the precedent exists; it just
   isn't applied to gameplay, particles, or timing yet.

3. **One primitive is universal.** All 11 games already speak exactly one shared word: a `postMessage`
   `"complete"` / outcome report at game end. It maps directly onto the existing `resolve → outbox` path —
   the seam the whole catalog is already built around.

---

## Vocabulary by domain

`n` = number of games (of 11) that demand the primitive.

### Rendering — 2D surface  *(the largest investment)*

| Primitive | What it does | Needed by | Axiom | n |
|---|---|---|---|---|
| Text / glyph / emoji | labels, HUD numbers, emoji-as-sprites | all 11 | missing | 11 |
| Alpha blending | per-draw transparency / fade | BC LL NI RH 1v1 PM NR | missing | 7 |
| Layer / z-order | explicit draw ordering | BC NC NI RH 1v1 PM NR | partial | 7 |
| 2D shapes | rect · arc · ellipse · line · path · bezier | BC NC NI RH 1v1 PM | missing | 6 |
| Progress bar / gauge | normalized fill bar | TF BC BB LL NI RH | missing | 6 |
| Gradients | linear & radial fills | BC NI RH 1v1 NR | missing | 5 |
| Glow / shadow | shadowBlur neon outlines | NI RH 1v1 PM | missing | 4 |
| Particle system | emitter · velocity · gravity · fade | NI RH 1v1 PM | missing | 4 |
| DPR + responsive resize | device-pixel scaling, reflow | RH 1v1 PM NR | partial | 4 |
| 2D transform stack | translate/rotate/scale, pivot-rotation | 1v1 PM NR | missing | 3 |
| Off-screen baked layer | render-to-texture / blit static layer | PM NR | missing | 2 |
| Sprite / image draw | PNG textures + pose-swap | 1v1 | missing | 1 |

### Rendering — 3D scene  *(Axiom's strength)*

| Primitive | What it does | Needed by | Axiom | n |
|---|---|---|---|---|
| Procedural geometry | box / sphere / cylinder primitives | NC NI NR | have | 3 |
| Perspective camera | fov/aspect/near/far projection | RH NR | have | 2 |
| 3D mesh raster | z-buffer, backface cull, perspective divide | NR | have | 1 |
| Materials | data-driven baseColor/emissive/roughness/opacity | NR | have | 1 |
| Lighting model | ambient + hemisphere + directional + fog | NR | partial | 1 |

### Spatial & math

| Primitive | What it does | Needed by | Axiom | n |
|---|---|---|---|---|
| clamp / lerp / sine / angle | scalar interpolation helpers | TF BC LL NC NI RH 1v1 PM NR | have | 9 |
| Grid / tilemap | 2D integer cell map | TF BC BB LL NI PM NR | missing | 7 |
| AABB / point-in-rect | box overlap hit test | BC NC NI 1v1 | partial | 4 |
| Circle / sphere overlap | distance-based hit test | LL 1v1 PM NR | partial | 4 |
| BFS pathfinding | shortest path / reachability | LL PM NR | missing | 3 |
| Vec / Mat4 / Quat | vector & matrix algebra | RH 1v1 NR | have | 3 |
| Tile↔pixel + center-snap | coordinate convert, snap-to-cell | LL PM | missing | 2 |
| Raycast | beam / ground probe | LL NR | have | 2 |
| Agent steering | best-first target selection | PM | missing | 1 |
| Offset-group translation | rigid formation moved as one body | NI | partial | 1 |

### Physics & motion

| Primitive | What it does | Needed by | Axiom | n |
|---|---|---|---|---|
| Euler integration | position += velocity·dt | NI RH 1v1 PM NR | partial | 5 |
| Gravity | constant downward accel | NI RH 1v1 PM NR | partial | 5 |
| Sinusoidal motion | oscillating AI / animation | NC NI 1v1 PM | have | 4 |
| Ballistic arc | bezier / projectile trajectory | 1v1 NR | partial | 2 |
| Jump arc | Vy + gravity + floor land | NC NR | partial | 2 |
| Tile-locked movement | grid-axis stepping | LL PM | missing | 2 |
| Constant-velocity scroll | uniform note/entity travel | NI RH | have | 2 |
| Knockback / impulse | one-shot velocity kick | NC 1v1 | partial | 2 |
| Rigid-body physics ⚠ | torque, impulse, damping, broadphase, solver | NR | partial | 1 |
| Friction / damping / brake | velocity decay | NR | missing | 1 |

### Time & lifecycle

| Primitive | What it does | Needed by | Axiom | n |
|---|---|---|---|---|
| Timer / countdown / cooldown | timestamp-gated events | all 11 | have | 11 |
| Spawn / despawn + pool | entity lifecycle, object pool | BB NC NI RH 1v1 PM NR | have | 7 |
| Game-flow state machine | menu / playing / over | NC NI RH 1v1 PM NR | partial | 6 |
| Variable-dt loop ⚠ | RAF with dt cap | NC NI RH 1v1 NR | partial | 5 |
| setTimeout sequencing ⚠ | no-loop async delay chains | TF BC BB LL RS | partial | 5 |
| Tween / easing | animated value transitions | TF BC BB LL RS | missing | 5 |
| Per-entity state machine | poses / AI modes / round phases | NC 1v1 PM NR | partial | 4 |
| Flip-book animation | frame-swap sprite cycles | NI 1v1 PM | missing | 3 |
| Fixed-step tick | deterministic simulation cadence | NC PM NR | have | 3 |
| Frame command queue | FIFO command bus | NR | missing | 1 |

### Input

| Primitive | What it does | Needed by | Axiom | n |
|---|---|---|---|---|
| Keyboard | state-map, edge detect, repeat suppress | RS LL NC NI RH 1v1 PM NR | have | 8 |
| Pointer / click | mouse/tap on elements & canvas | TF BC BB RS LL RH | have | 6 |
| Touch / swipe / gesture | mobile directional input | LL BB RH PM | have | 4 |
| Key→action bindings | data-driven remappable controls | NC NI RH NR | partial | 4 |
| Charge / hold-release | power-meter input | NI 1v1 | partial | 2 |
| Buffered direction | queued turn at next cell | NC PM | missing | 2 |
| Timing-window hit detect | beat-accurate input judging | RH | missing | 1 |
| Two-phase modal input | arm-then-fire | LL | missing | 1 |

### Randomness & procedural generation

| Primitive | What it does | Needed by | Axiom | n |
|---|---|---|---|---|
| Fisher-Yates shuffle | unbiased permutation | TF LL PM NR | partial | 4 |
| Seeded PRNG | reproducible random stream | NC PM NR | have | 3 |
| Weighted pick | probability-table selection | BB NC | partial | 2 |
| Maze generation | DFS backtracker + mirror + overlay | PM | missing | 1 |
| Grid→geometry pipeline | drunkard-walk · BFS · L-system turtle · audit | NR | missing | 1 |
| Solvable level gen | rejection-sampling with guarantees | LL | missing | 1 |
| Noise / data-texture | procedural texture sampling | NR | partial | 1 |

### Audio  *(0 of these exist in Axiom today)*

| Primitive | What it does | Needed by | Axiom | n |
|---|---|---|---|---|
| Audio-clock scheduling | sample-accurate timing | RS RH PM | missing | 3 |
| Synthesis | oscillator + gain + envelope | RS PM | missing | 2 |
| Mute / volume / persistence | output control | PM NR | missing | 2 |
| LFO modulation | frequency-modulated tones | PM | missing | 1 |
| Sample / playlist playback | music file streaming | NR | missing | 1 |
| Live capture + FFT ⚠ | mic/system audio → frequency bands | RH | missing | 1 |

### UI / HUD / chrome

| Primitive | What it does | Needed by | Axiom | n |
|---|---|---|---|---|
| Overlay screens / modals | menu / pause / result, state-driven | all 11 | missing | 11 |
| Responsive layout solver | adaptive board/panel placement | TF BB LL RH 1v1 PM NR | have | 7 |
| Canvas-drawn HUD | in-surface stat readouts | NC NI RH 1v1 PM | missing | 5 |
| Stat / leaderboard / awards | DOM data panels | NI RH 1v1 PM | missing | 4 |
| Floating popups / toasts | ephemeral score labels | NI 1v1 PM | missing | 3 |
| Loading / progress screen | async build progress bar | NR | partial | 1 |
| Markdown rendering | credits / rich text | NR | missing | 1 |

### Integration & persistence  *(the embed seam)*

| Primitive | What it does | Needed by | Axiom | n |
|---|---|---|---|---|
| Outcome postMessage | `complete` / gameMetrics to parent | all 11 | partial | 11 |
| localStorage | best score / leaderboard / mute | BB RS LL NI 1v1 PM NR | missing | 7 |
| fetch record-gameplay | same-origin server result POST | TF BC RS NI 1v1 PM | partial | 6 |
| URL-param config injection | uid / prize / threshold at init | BB NC NI 1v1 NR | partial | 5 |
| External reward / webhook POST | direct points / webhook call | BB NI 1v1 | partial | 3 |
| postMessage capability bridge | request/response to parent frame | NI 1v1 | missing | 2 |
| JWT handshake (origin-checked) | widget token in via postMessage | NC BB | partial | 2 |
| WebSocket realtime | snapshot / delta / ack protocol | NC | have | 1 |

---

## Three games that don't fit the mould

- **neon-clash — the template.** The only already-authoritative game: server-side on `DataDrivenGame`
  with a fixed-step tick, data-driven per-entity state machine, 1-D physics, hitbox windows, seeded
  weighted RNG, and a WebSocket snapshot/delta protocol. This is the authority model to **generalise**,
  not replace.

- **nova-roll — the heavy lift.** Ships its own CPU software 3D rasterizer (Axiom replaces this outright)
  and depends on `cannon-es` rigid-body physics — torque, impulse, friction, broadphase — beyond Axiom's
  current sphere/box/plane module. Its procgen is already deterministic; its float physics is not.

- **rhythm-hero — the misfit.** Has **no authored chart**: notes are generated live from microphone /
  system-audio FFT via `getUserMedia` / `getDisplayMedia`. Non-deterministic external input by design.
  It cannot be server-authoritative without a redesign to authored charts — or it stays a client-only,
  non-authoritative title. **This is a decision that must be made explicitly.**

---

## Net position: what Axiom has vs. what this catalog forces it to grow

### Already in the tree
- 3D scene graph — meshes, materials, textures, lights, perspective camera *(nova-roll)*
- Vec / Mat4 / Quat / Transform math
- Raycast + overlap-box spatial queries *(Vocabulary Category 2, landed)*
- spawn / despawn entity lifecycle *(Category 3, landed)*
- Fixed-step deterministic tick + seeded RNG + replay/state-hash
- WebSocket netcode: snapshot / delta / ack *(neon-clash)*
- Input intent synthesis + responsive layout solver

### Must be added (ranked by reach)
1. **A 2D surface** — sprites, shapes, text, gradients, transforms, particles *(9 games)*
2. **Tilemap / grid** + tile-center movement *(7 games)*
3. **An audio subsystem** — synthesis, sample/playlist, mute *(4 games; 0 today)*
4. **BFS pathfinding** + agent steering *(last-light, pointman, nova-roll)*
5. **Tween / easing** + flip-book animation
6. **Physics gaps** — friction, torque, angular, damping *(nova-roll)*
7. **HUD / overlay model** + the outcome-report seam, standardised
8. **The TS authoring API** that projects all of the above to game devs

---

## Cross-cutting: the determinism hazards (⚠)

Every game except neon-clash must be moved off these before it can run on a deterministic,
replayable, authoritative engine:

- **Unseeded `Math.random`** — present in 9 of 11 games. Fix: route through the engine's seeded RNG
  (the `Seeded PRNG` primitive, already `have`). pointman and nova-roll already do this for procgen.
- **Wall-clock timing** — `Date.now` / `performance.now` / `setTimeout` / `setInterval`. Fix: the
  engine clock + fixed-step tick.
- **Variable-dt RAF loops** — frame-rate-dependent motion. Fix: fixed-step accumulator.
- **Float rigid-body physics (`cannon-es`)** — cross-platform non-determinism *(nova-roll)*.
- **Live audio FFT** — external, irreproducible input *(rhythm-hero; see misfit above)*.
