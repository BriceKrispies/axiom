# Arena Forge — Game Rules

The complete player-facing and authoritative rules of an eight-player Arena
Forge match, as actually implemented. Every number below is read directly
from `src/sim/tuning.ts`'s `DEFAULT_RULES` (`rulesVersion: 1`); a modified
`Rules` object can be passed to `Match`/harness calls for testing, but
production always uses `DEFAULT_RULES`.

## 1. The phase machine

```text
lobby ──────► shop ──────► combat_prepare ──────► combat ──────► combat_resolve ──────► round_transition ──┬──► shop  (next round)
                ▲                                                                                            │
                └────────────────────────────────────────────────────────────────────────────────────────────┘
round_transition ──► match_complete   (terminal — 1 player remains, or a placement-1 champion was decided)
```

The legal edges, verbatim from `sim/phase.ts` (`LEGAL_TRANSITIONS`) — the
independent specification the harness checks the produced transition log
against, not just documentation of intent:

| From | May go to |
|---|---|
| `lobby` | `shop` |
| `shop` | `combat_prepare` |
| `combat_prepare` | `combat` |
| `combat` | `combat_resolve` |
| `combat_resolve` | `round_transition` |
| `round_transition` | `shop`, `match_complete` |
| `match_complete` | *(none — terminal)* |

Any other `from → to` pair is illegal; `isLegalTransition` is what the 100-match
suite checks every produced transition against (0 illegal transitions is an
asserted invariant).

`round` is 0 at `lobby` and increments by 1 every time `beginShop()` runs, so
round 1 is the first shop phase. `combat_prepare` and `combat` (and their
resolution) all happen at the current round number; `round_transition` is a
one-tick bookkeeping phase, not a player-visible pause.

Match lifecycle, matching `Match` (`sim/match.ts`):

- **`start()`** — from `lobby`, begins round 1's shop.
- **`tick()`** — advances the authoritative tick counter by 1; if the current
  phase is `shop` or `combat` (the two *timed* phases) and `tick >=
  phaseDeadlineTick`, automatically calls `advancePhase()`.
- **`advancePhase()`** — force-ends the current timed phase now (used by tests
  and the headless harness for deterministic accelerated play; production
  paces itself via `tick()`).
  - from `shop`: runs `enterCombat()` — computes and simulates every pairing's
    combat, then transitions into `combat` for its playback window.
  - from `combat`: runs `resolveCombat()` — applies consequences, updates
    stages, and either starts the next shop or ends the match.

## 2. Economy

Eight players start a match. Every value below is `DEFAULT_RULES`:

| Quantity | Value |
|---|---|
| Starting health | 30 |
| Starting gold | 3 (but see gold-by-round below — round 1's grant overwrites this) |
| Starting forge rank | 1 |
| Max forge rank | 6 |
| Max gold (hard cap on any gold-adding effect) | 10 |
| Reroll cost | 1 gold |
| Sell value | 1 gold (flat, refunded on any sell) |
| Hand limit | 6 cards |
| Warband limit | 7 slots (`WARBAND_SLOTS`, `model.ts`) |
| Copies to forge | 3 normal copies of one card |
| Default forge reward | `{ kind: "gold", amount: 1 }` (overridable per-card via `forgeReward`) |
| Shop timer | 45 seconds (× `FIXED_HZ` = 30 → 1350 ticks) |
| Combat playback window | 12 seconds (× 30 → 360 ticks) |

**Gold granted at the start of round N** (`goldByRound`, 1-indexed, clamped to
`maxGold` and to the last array entry for rounds past its length):

| Round | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8+ |
|---|---|---|---|---|---|---|---|---|
| Gold | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |

**Forge rank upgrade cost** (`forgeRankUpgradeCosts`, cost to go from rank
`r` to `r+1`; `null`/unavailable once at rank 6):

| From rank | 1→2 | 2→3 | 3→4 | 4→5 | 5→6 |
|---|---|---|---|---|---|
| Cost | 5 | 6 | 7 | 8 | 9 |

**Shop size by forge rank** (`shopSizeByRank`):

| Forge rank | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|
| Cards offered | 3 | 4 | 4 | 5 | 5 | 6 |

**Tier roll weights by forge rank** (`tierWeightsByRank` — a weight of `0`
means that tier cannot appear at that rank; weights are relative, not
percentages, and a tier's *effective* weight is forced to 0 if the pool has
no remaining stock of that tier regardless of the table):

| Rank | T1 | T2 | T3 | T4 | T5 | T6 |
|---|---|---|---|---|---|---|
| 1 | 100 | 0 | 0 | 0 | 0 | 0 |
| 2 | 70 | 30 | 0 | 0 | 0 | 0 |
| 3 | 45 | 35 | 20 | 0 | 0 | 0 |
| 4 | 30 | 33 | 25 | 12 | 0 | 0 |
| 5 | 20 | 28 | 27 | 18 | 7 | 0 |
| 6 | 14 | 22 | 25 | 22 | 12 | 5 |

**Every shop-phase player action** goes through `Match.submit(playerId,
command)` → `applyShopCommand` (`sim/economy.ts`) as one of eight commands
(`sim/commands.ts`): `buy`, `sell`, `reroll`, `set_freeze`,
`upgrade_forge_rank`, `play_card`, `return_to_hand`, `reorder`. Commands are
accepted **only** during `shop`, only from a known, non-eliminated player.
Every command is fully validated before any mutation — a rejected command
changes nothing and emits an explicit `command_rejected` event carrying a
stable reason string (`badShopIndex`, `notEnoughGold`, `handFull`,
`warbandFull`, `slotOccupied`, `badSlot`, `unknownInstance`, `notInHand`,
`notInWarband`, `maxForgeRank`, `nothingToReturn`, `noChange`, plus phase/player
gate reasons `wrongPhase`, `player_eliminated`, `unknown_player`).

At the start of every shop phase (`beginShop`, `match.ts`), for every active
player in order: grant that round's gold, refresh the shop (unless
`shopFrozen`, which is consumed — one freeze covers exactly one refresh),
fire the `shop_start` economy trigger for every warband unit, and update the
player's presentation stage.

## 3. Forging

Whenever a player holds **3 normal (unforged) copies** of one card across
hand + warband, they combine automatically and deterministically
(`resolveForges`/`forgeOnce`, `sim/forge.ts`) — this check runs after every
`buy` and every `play_card`. There is no card-specific forging code: the rule
reads a card's own `forgedStats`, `forged` ability list, `forgedVisualProfile`,
and optional `forgeReward`.

- **Which copies combine**: the canonical card order (`content.collectibleCards`,
  sorted `(tier, id)`) decides which card forges first if a player has
  multiple forgeable sets at once. For one card, its copies are gathered
  warband (slot order) then hand (index order), and the *first three* in that
  order are consumed.
- **Destination (deterministic)**: the leftmost of the three consumed copies'
  original warband slot, if it was in the warband; otherwise the first empty
  warband slot; otherwise the hand.
- **Forged stats**: `attack = baseAttack + forgedStats.attack`, `health =
  baseHealth + forgedStats.health` (flat bonuses on top of base — not on top
  of any temporary buffs the three consumed copies had accrued).
- **Ability profile**: the forged unit's `forged: true`, so every trigger
  lookup (`runEconomyTrigger`/`runTrigger`) reads the card's `forged` ability
  array instead of `normal`, and `visualStage` becomes `1`.
- **Forge reward**: granted once, immediately — the card's own `forgeReward`
  if set, else `DEFAULT_RULES.defaultForgeReward` (`{ gold, amount: 1 }`). A
  `gold` reward adds gold (clamped to `maxGold`); a `discount` reward reduces
  the cost of every card currently in the player's shop (clamped to each
  card's own cost, floored at 0).
- **Bounded**: `resolveForges` loops until no more forges are possible, capped
  at `EFFECT_BOUNDS.maxSummonsPerCombat` (60) iterations as a generous
  backstop — a forge can create a new set of 3 (e.g. a reward that grants a
  copy), so the loop is not a single pass.

## 4. Combat

Combat for an entire round is computed **synchronously and deterministically**
at `combat_prepare` (`runCombat`, `combat/engine.ts`); the `combat` phase is
pure event-stream playback (see `ARCHITECTURE.md`).

- **Immutable snapshot.** Each side is built from a `WarbandSnapshot`
  (`snapshotWarband`, `combat/board.ts`) — a deep copy of the warband taken
  the instant `combat_prepare` runs. Combat mutates only the transient
  `CombatUnit` copies on the `Board`; it never writes back to `PlayerState`.
- **Per-combat seed.** `deriveSeed(matchSeed, round, combatId)` — an
  independent, reproducible sub-stream per fight.
- **Initiative.** `env.rng.chance(1, 2)` picks side `a` or `b` to attack
  first. Sides then alternate one attack action at a time.
- **Attacker selection.** The leftmost living unit on the acting side that has
  not yet attacked *this cycle*; once every living unit on that side has
  attacked, every unit's `hasAttacked` flag resets and the cycle restarts from
  slot 0 (`nextAttacker`, `engine.ts`).
- **Defender selection.** The leftmost living enemy — **unless** any enemy has
  `guard`, in which case the pool is restricted to guard units only, and the
  leftmost of those is chosen (`chooseDefender`).
- **Damage is simultaneous.** Both hits are applied before either side's death
  is checked: `applyDamage(defender, attacker.attack)` then
  `applyDamage(attacker, defender.attack)` are both computed in
  `resolveAttack` before `resolveDeaths` runs — an attacker and defender can
  trade lethal blows and both die from the same exchange.
- **Armored mitigation.** A unit with the `armored` keyword reduces *each*
  incoming hit by 1, floored at 0 (`applyDamage`, `combat-effects.ts`):
  `dealt = max(0, amount - 1)`.
- **Death resolves before the next attack.** After every ability's operations
  and after every attack, `resolveDeaths` (`combat-effects.ts`) runs to a
  fixed point: collect every unit with `health <= 0`, mark it dead, emit
  `unit_died`, fire its `on_death` trigger, clear its slot — and repeat, since
  a deathrattle can kill something else, until no unit is left at `health <=
  0` (or a bound is hit).
- **Summon placement.** `summon_token` resolves its `at` selector; when that
  selector is (as authored today) `{ kind: "empty_friendly_slot" }`, it
  intentionally resolves to no unit (there is no board-slot selector kind),
  so the summon anchors at the **source unit's own slot**, and the token is
  placed at the empty slot on that side *nearest* that anchor
  (`nearestEmptySlot`, ties resolve toward slot 0). If the side is full, the
  summon silently does nothing.
- **Bounds → diagnostic draw.** Hard runtime caps stop any pathological
  ability from hanging combat (`EFFECT_BOUNDS`, `effects/language.ts`,
  enforced via `CombatCounters`, `combat/env.ts`):

  | Cap | Value |
  |---|---|
  | Max events per combat | 6000 |
  | Max token summons per combat | 60 |
  | Max `copy_ability` resolutions per combat | 24 |
  | Max attack actions per combat | 500 |

  Hitting any cap calls `terminate(env, reason)`, which emits a `diagnostic`
  event and force-ends the combat as a **draw**: `winnerSide = null`, both
  verdicts `"draw"`, `survivors = 0`, `winnerForgeRank = 0`,
  `survivingTierSum = 0`. The 100-match suite tracks this as `boundCombats`
  (0 observed across the standard 100-seed suite with default content).
- **Combat ends** the instant one side has zero living units (or a bound
  fires). Winner-side survivors, the winner's `forgeRank`, and the sum of the
  winner's surviving units' tiers are captured for the consequence formula.

## 5. Consequences (round resolution)

`applyRoundResolution` (`sim/resolution.ts`) turns each round's combat results
into player health changes, all **after every combat in the round has
completed** (never mid-round), then eliminates and places players
simultaneously.

**Base loss formula**, clamped to `maxConsequence` (15):

```text
base = clamp(0, 15, surviving_enemy_count + opponent_forge_rank + sum_of_surviving_enemy_tiers)
```

Only the **loser** of a real combat takes `base` damage; the winner takes 0.

**Anti-stalemate escalation** — guarantees every match terminates even if two
static boards would otherwise mutually stalemate forever:

```text
escalation = max(0, round - 8) * 2        // escalationStartRound=8, consequenceEscalation=2
```

`escalation` is **added after the clamp** to a loss (`base + escalation`), and
is **also dealt to both players on a draw** (a drawn combat deals `escalation`
to each side once `round > 8`, even though a draw deals 0 before then). A win
still deals 0. Concretely: round 9 adds 2 damage, round 10 adds 4, round 11
adds 6, and so on, unbounded — so no round past 8 is ever damage-free for a
loser or a drawn pair.

A ghost fight and a true bye never deal or take **player** health damage on
the ghost/bye side: `pairing.b === null` for both cases, and
`computeOutcomes` only ever produces a pending-damage entry for a pairing's
`b` side when `pairing.b !== null` — there is no live player behind a ghost
or a bye to damage. A true bye (no ghost available) is a scripted, guaranteed
win: `byeResult` fabricates a win using the player's *own* board as both the
survivor count and tier sum, so the bye is a free win worth exactly what that
player's current board would be worth against a mirror of itself.

## 6. Elimination and placement

- Damage from every pairing is applied together, after all of the round's
  combats resolved.
- Every player left at `health <= 0` is eliminated **simultaneously** in the
  same round — there is no ordering advantage to dying "first."
- **Placement tiebreak**, applied to every player eliminated this round,
  best-placement (lowest number) first, verbatim from the comparator in
  `applyRoundResolution`:
  1. **Higher post-damage health** wins (a player who ended the round at −1
     outranks one who ended at −5).
  2. If tied, **more damage received this round** wins. This looks
     counter-intuitive next to tiebreak 3, but is self-consistent: if two
     players end at the *same* final health, the one who absorbed more
     damage necessarily started the round with more health — this term is a
     same-round proxy for tiebreak 3, resolved before falling back to it.
  3. If still tied, **higher pre-combat (start-of-round) health** wins.
  4. If still tied, **lower stable player id** wins (a fully deterministic
     final tiebreak — never a coin flip).
- Placements are assigned counting down from the number of currently-active
  players: if 8 are active and 2 are eliminated this round, they receive
  placements 7 and 8 (worse numbers = eliminated earlier/worse).
- **Match end**: when exactly one player remains, they are placement 1 and
  the match winner. In the rare case every remaining active player is
  eliminated in the same final round (mutual wipe), the placement-1 player
  from that round's tiebreak is declared the winner.
- Every eliminated player's final warband is snapshotted (`ghostStore
  .snapshots`) for possible future use as a ghost.

## 7. Opponent pairing

Recomputed every round from the currently-active player set
(`computePairings`, `sim/pairing.ts`):

1. **Reseed.** Active players are sorted: health desc, forge rank desc,
   warband power desc (`attack + health` summed across the warband,
   `warbandPower`, `stage.ts`), stable id asc.
2. **Odd count → bye/ghost slot.** If the reseeded list has an odd length,
   the **lowest-seeded** player (popped off the end) does not face a live
   opponent this round.
3. **Pair adjacently**, `(order[0], order[1]), (order[2], order[3]), …`
4. **Rematch avoidance (local swap only).** If a pair's `a` player's
   `lastOpponent` is the paired `b`, the algorithm searches forward for the
   nearest later player `j` such that neither `a` nor `order[j]` last fought
   each other, and swaps `order[i+1]` with `order[j]`. If no valid swap
   exists, the immediate rematch stands rather than breaking determinism —
   this is a best-effort, deterministic local repair, not a global
   re-optimization.
5. **Ghost selection.** For the odd-one-out, the ghost is the
   **highest-placing** (lowest placement number) eliminated player who: has a
   stored warband snapshot, and was **not** the ghost used last round
   (`ghostUsedLastRound`). If no eligible eliminated player exists, that slot
   is a true bye (a guaranteed win, see §5) instead of a ghost fight. A ghost
   never takes player damage (its "owner" already has no player-health state
   to affect — see §5).

## 8. Arena presentation stages

A purely-derived, data-driven function of a player's current forge rank,
number of forged units in the warband, and warband power (`computeStage`,
`sim/stage.ts`). Thresholds are checked strongest-first — the first one a
player meets wins (`stageThresholds`, `tuning.ts`):

| Stage | Forge rank ≥ | Forged units ≥ | Warband power ≥ |
|---|---|---|---|
| `masterwork` | 5 | 3 | 60 |
| `tempered` | 4 | 2 | 36 |
| `kindled` | 2 | 1 | 16 |
| `workshop` | 1 | 0 | 0 |

Every player starts at `workshop`. A change is recomputed after every
command that could move a player's stats (`updateStage`, `match.ts`) and at
`shop_start`/`combat_resolve`, emitting a `stage_changed` event on transition.
The stage carries no visual data itself — it is an id the (not-yet-built)
presentation layer would map to actual scenery/lighting.

## 9. Termination guarantee

`DEFAULT_RULES.maxRounds = 60` is a hard diagnostic ceiling (used by the
harness to flag a runaway match), but the real termination guarantee is
structural: every round strictly reduces the active player count *or* every
loss/draw deals damage once `round > escalationStartRound` (8) — so health
totals are strictly bounded and monotonically decreasing across enough
rounds, and the match cannot stall indefinitely. The 100-match suite observes
an average of ~13 rounds per match with the default content and bot
policies, and zero matches exceeding the round cap.
