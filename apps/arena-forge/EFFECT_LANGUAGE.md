# Arena Forge — Effect Language Reference

The complete, typed, declarative language every Arena Forge card is authored
in (`sim/effects/language.ts`). Cards carry data, never code. This document
enumerates every `kind` string the language and its two interpreters
(`sim/effects/economy-effects.ts` for the economy context,
`sim/combat/combat-effects.ts` for the combat context) support, exactly as
implemented — nothing here is aspirational.

## Triggers and their execution context

An ability's `trigger` fixes which interpreter runs it — there is no card
override. `TRIGGER_CONTEXT` (`language.ts`) is the source of truth:

| Trigger | Context | Fires when |
|---|---|---|
| `on_buy` | economy | A unit is purchased (to hand or warband) |
| `on_play` | economy | A unit enters the warband (bought straight to warband, or `play_card` from hand) |
| `on_sell` | economy | A unit is sold |
| `shop_start` | economy | Every shop phase begins, for every living warband unit |
| `round_end` | economy | *(declared in the language; not currently emitted by any Match code path — see note below)* |
| `combat_start` | combat | Combat begins, before the first attack — fires for every living unit, side `a` then `b`, in slot order |
| `before_attack` | combat | Immediately before an attacker's hit is applied |
| `after_attack` | combat | Immediately after simultaneous damage is applied |
| `on_damage` | combat | A unit actually took damage > 0 this exchange (defender checked first, then attacker) |
| `on_survive_damage` | combat | A unit took damage this exchange and is still alive with health > 0 (defender checked first, then attacker) |
| `on_death` | combat | A unit's health drops to ≤ 0 (fires during `resolveDeaths`, the fixed-point death resolver) |
| `on_friendly_summon` | combat | Any unit is summoned on the same side (fires for every OTHER living unit on that side, in slot order) |
| `on_enemy_summon` | combat | Any unit is summoned on the opposing side (fires for every living unit on the summoning side's enemy, in slot order) |
| `passive_aura` | combat | Once at `combat_start`, immediately after every unit's `combat_start` abilities have fired |

Note on `round_end`: it is a fully valid, validator-legal trigger kind (part
of `ALL_TRIGGERS`, economy context, and every economy operation lists it as
legal via `OPERATION_CONTEXT`), but no card in the current roster uses it and
no `Match`/economy code path currently fires it — it is a reserved trigger
point, not dead code, since content authoring and validation both already
support it.

## Conditions

Every `Condition` reads only explicit unit/board/round state — never wall
clock, never randomness. All conditions on an ability are AND-ed; any single
failing condition skips the whole ability.

| `kind` | Meaning |
|---|---|
| `source_is_forged` | The ability's own unit is forged |
| `source_is_normal` | The ability's own unit is not forged |
| `source_attack_at_least` `{ value }` | Source's current attack ≥ value |
| `source_health_at_least` `{ value }` | Source's current health ≥ value |
| `source_in_group` `{ group }` | Source's card belongs to `group` |
| `source_has_keyword` `{ keyword }` | Source has `keyword` (card-defined or granted) |
| `source_position_leftmost` | Source is the leftmost living unit on its side |
| `source_position_rightmost` | Source is the rightmost living unit on its side |
| `adjacent_in_group` `{ group }` | Either immediate neighbor (slot ± 1) belongs to `group` |
| `round_at_least` `{ value }` | Match round ≥ value. **In combat this always evaluates true** (`conditionHolds` in `combat-effects.ts`: "round is not a combat input; treated as satisfied in combat") — only meaningful for economy-context abilities |
| `friendly_group_count_at_least` `{ group, value }` | Count of living friendly units in `group` ≥ value |
| `empty_warband_slots_at_least` `{ value }` | Count of empty warband slots on the source's side ≥ value |

## Selectors

Every `Selector` resolves deterministically against the current board (combat
context) or warband (economy context). "Friendly"/"enemy" are relative to the
ability's source. Random selectors draw from the match's single seeded `Rng`
in stable slot order — reproducible given the seed.

| `kind` | Resolves to (combat) | Resolves to (economy) |
|---|---|---|
| `self` | The source unit, if alive | The source unit |
| `attacker` | The current attack's attacker (via `Focus`), if alive | *(no economy meaning — resolves empty)* |
| `defender` | The current attack's defender (via `Focus`), if alive | *(no economy meaning — resolves empty)* |
| `adjacent_friendly` | Living neighbor(s) at slot ± 1 on source's side | Same, over the warband |
| `leftmost_friendly` | Leftmost living friendly unit | Leftmost warband unit |
| `rightmost_friendly` | Rightmost living friendly unit | Rightmost warband unit |
| `random_friendly` | One random living friendly unit (`rng.pick`) | One random warband unit |
| `random_enemy` | One random living enemy unit | *(no economy meaning — resolves empty)* |
| `lowest_attack_friendly` | Friendly with lowest attack (ties broken by instance id asc) | Same, over the warband |
| `highest_attack_enemy` | Enemy with highest attack (ties broken by instance id asc) | *(no economy meaning — resolves empty)* |
| `all_friendly` | Every living friendly unit | Every warband unit |
| `all_enemy` | Every living enemy unit | *(no economy meaning — resolves empty)* |
| `friendly_in_group` `{ group }` | Living friendlies in `group` | Warband units in `group` |
| `empty_friendly_slot` | *(no unit — falls to the default "resolves empty" case; see `summon_token` below for how this is actually used)* | *(same — resolves empty)* |

An economy-only or combat-only selector resolves to an **empty list** in the
wrong context rather than erroring — the operation it feeds simply affects
zero units.

## Operations (the verbs)

| `kind` | Shape | Effect |
|---|---|---|
| `modify_attack` | `{ target, amount }` | `target.attack = max(0, target.attack + amount)` for every resolved unit |
| `modify_health` | `{ target, amount }` | Combat: `target.health += amount` (no floor here — this is a buff/debuff op, not damage). Economy: `target.health = max(1, target.health + amount)` (a living unit's health floors at 1 outside combat) |
| `deal_damage` | `{ target, amount }` | Combat only — routes through `applyDamage` (armor-mitigated; see below) |
| `heal` | `{ target, amount }` | Combat only — `target.health += amount`, uncapped |
| `grant_keyword` | `{ target, keyword }` | Adds `keyword` to the target's granted-keywords list (idempotent — no duplicate) |
| `remove_keyword` | `{ target, keyword }` | Removes `keyword` from the target's granted-keywords list |
| `summon_token` | `{ token, at, count }` | Combat only — see "Summon placement" below |
| `move_unit` | `{ target, to: "leftmost" \| "rightmost" }` | Combat only — relocates the target to the nearest empty slot searching from that end |
| `swap_with` | `{ target, other }` | Combat only — exchanges board positions of two units on the **same side**; no-ops if either resolves empty, they're equal, or they're on different sides |
| `copy_ability` | `{ from }` | Combat only — runs the donor's own `combat_start` ability's operations (if any) as if the source cast them; bounded (see below) |
| `repeat` | `{ times, op }` | Runs `op` `min(times, maxRepeat)` times; the one recursive shape in the language, bounded by nesting depth at validation and by a live `terminated` check in combat |
| `add_gold` | `{ amount }` | Economy only — `gold = min(maxGold, gold + amount)` |
| `discount_shop` | `{ amount }` | Economy only — reduces every current shop slot's price by `amount`, floored per-card at that card's own cost (i.e. never below 0) |
| `transform_card` | `{ target, into }` | Changes the target's `cardId` to `into` and resets/recomputes its stats from the new definition (economy: resets forged/keywords/visualStage to normal; combat: recomputes attack/health from the new card's base + forged bonus if the unit is currently forged, keeping `forged` status) |
| `emit_cue` | `{ cue }` | Pure presentation signal — emits a `cue` `SimEvent` (combat) or is a no-op (economy interpreter treats it, and any other unrecognized/combat-only op, as a silent no-op) |

### Operation-context legality (`OPERATION_CONTEXT`)

Which verbs are legal in which trigger context — enforced at content-load
time by the validator (an economy-only op authored on a combat trigger, or
vice versa, is a load error):

| Operation | economy | combat |
|---|---|---|
| `modify_attack` | ✓ | ✓ |
| `modify_health` | ✓ | ✓ |
| `deal_damage` | | ✓ |
| `heal` | | ✓ |
| `grant_keyword` | ✓ | ✓ |
| `remove_keyword` | ✓ | ✓ |
| `summon_token` | | ✓ |
| `move_unit` | | ✓ |
| `swap_with` | | ✓ |
| `copy_ability` | | ✓ |
| `repeat` | ✓ | ✓ |
| `add_gold` | ✓ | |
| `discount_shop` | ✓ | |
| `transform_card` | ✓ | ✓ |
| `emit_cue` | ✓ | ✓ |

## Validation bounds (`EFFECT_BOUNDS`, `language.ts`)

Checked at content-load time by `validateContent` — a card that exceeds any
of these fails validation and the content bundle throws:

| Bound | Value | Enforces |
|---|---|---|
| `maxRepeat` | 8 | Max `times` on any single `repeat` operation |
| `maxSummonPerOperation` | 6 | Max `count` on any single `summon_token` operation |
| `maxOperationDepth` | 3 | Max nesting depth of `repeat` within `repeat` |
| `maxAbilitiesPerProfile` | 6 | Max abilities in a card's `normal` array, and separately its `forged` array |
| `maxOperationsPerAbility` | 8 | Max operations in one ability's `operations` array |

Checked at **runtime** by the combat interpreter (`CombatCounters`,
`combat/env.ts`) — no authored card, however pathological (e.g. a summon
chain that keeps triggering itself), can hang or infinite-loop a combat:

| Bound | Value | Enforces | On breach |
|---|---|---|---|
| `maxEventsPerCombat` | 6000 | Total combat events emitted | `terminate(env, "event_bound")` |
| `maxSummonsPerCombat` | 60 | Total `summon_token` resolutions (also reused as the death-resolution fixed-point loop's iteration guard) | `terminate(env, "summon_bound")` |
| `maxCopiedAbilities` | 24 | Total `copy_ability` resolutions | `terminate(env, "copy_bound")` |
| `maxCombatActions` | 500 | Total attack actions | `terminate(env, "action_bound")` |

`terminate` is idempotent (keeps the first reason), emits a `diagnostic`
event, and flips `env.counters.terminated`, which every subsequent
`applyOperation`/`cemit` call checks and short-circuits on — the combat then
ends as a draw (see `GAME_RULES.md` §4).

## Deterministic ordering rules

1. **Abilities fire in authored order.** For one trigger firing on one unit,
   `runEconomyTrigger`/`runTrigger` iterate the unit's active profile's
   ability array (`normal` or `forged`) top to bottom, running every ability
   whose `trigger` matches.
2. **Board-wide triggers fire in slot order.** `shop_start`/`round_end`
   (`runBoardEconomyTrigger`) iterate the warband slot 0→6; `combat_start`/
   `passive_aura` (`fireStart`, `combat/engine.ts`) iterate side `a` fully
   (slot order) then side `b` fully; summon-reaction triggers
   (`on_friendly_summon`/`on_enemy_summon`) iterate `living(board, side)`,
   which filters the slot array in order.
3. **Random selectors draw from the combat's single seeded `Rng` in stable
   order.** Because ability firing order is itself deterministic (rules 1–2),
   every `rng.pick`/`rng.range` call happens at a reproducible point in the
   sequence — replaying the same seed reproduces the same random picks.
4. **Death resolution runs to a fixed point before the next attack.**
   `resolveDeaths` is called after every ability's operations
   (`runAbility`) and again after every full attack exchange
   (`resolveAttack`) — a deathrattle that kills a second unit is itself
   resolved (its `on_death` fires, its slot clears) before control returns to
   the attack loop.
5. **The exact attack-exchange trigger order** (`resolveAttack`,
   `combat/engine.ts`):

   ```text
   before_attack(attacker)
     → [abort this exchange if either unit died from before_attack's operations]
     → simultaneous damage: applyDamage(defender, attacker.attack); applyDamage(attacker, defender.attack)
     → after_attack(attacker)
     → on_damage(defender)   [only if defender.dealt > 0]
     → on_damage(attacker)   [only if attacker.dealt > 0]
     → on_survive_damage(defender)  [only if defender still alive AND health > 0]
     → on_survive_damage(attacker)  [only if attacker still alive AND health > 0]
     → resolveDeaths (fixed point) — each dying unit's on_death fires here, side "a" units before side "b" units
   ```

   And once at the very start of combat, before any attack:
   `combat_start` (every unit, side `a` then `b`) → `passive_aura` (every
   unit, side `a` then `b`) → `resolveDeaths`.

## Keyword semantics

Only two keywords carry mechanical meaning (`sim/content/keywords.ts`) — every
other unit identity in the content set comes from authored abilities, never
from an invented keyword:

- **`guard`** (`GUARD`, `combat/board.ts`) — `chooseDefender`
  (`combat/engine.ts`) restricts the defender pool to living guard units on
  the target side, if any exist; only when no guard is alive does it fall
  back to "leftmost living enemy." A guard unit does not otherwise change how
  it attacks or is targeted once no guards remain.
- **`armored`** (`ARMORED`) — `applyDamage` (`combat/combat-effects.ts`)
  reduces every incoming hit against an armored unit by exactly 1, floored at
  0 net damage: `dealt = max(0, amount - 1)`. This applies to `deal_damage`
  operations and ordinary attack damage alike — anything that routes through
  `applyDamage`.

Both keywords can be a card's innate `keywords` (schema-level, permanent) or
`grant_keyword`ed temporarily onto a unit at runtime — `unitKeywords`
(`combat/board.ts`) always unions both sources when checking for a keyword.
