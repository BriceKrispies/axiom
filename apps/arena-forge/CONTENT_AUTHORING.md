# Arena Forge — Content Authoring

How to add a new group, unit, token, visual profile, and forged ability
**without touching the match engine.** Every card is DATA — a
`CardDefinition` record in the effect language (`sim/effects/language.ts`).
There are no callbacks, no `eval`, no `new Function`, anywhere in the content
files (`sim/content/`). The interpreter (`sim/effects/economy-effects.ts`,
`sim/combat/combat-effects.ts`) switches on a fixed set of `kind` strings —
that switch is generic mechanism, not per-card branching. Adding a card never
touches the interpreter; adding a genuinely new verb (a new `Operation.kind`,
`Selector.kind`, etc.) is the one thing that does, and is a deliberate
language change (see `EFFECT_LANGUAGE.md`), reviewed as such, not routine
content work.

## Where content lives

```text
sim/content/
  schema.ts            the typed shapes (CardDefinition, GroupDefinition, VisualProfile, …)
  validate.ts           validateContent(bundle) — every content error class
  load.ts                LoadedContent — canonicalizes + indexes a validated bundle
  bundle.ts              assembles every file below into ContentBundle + loadDefaultContent()
  archetypes.ts           ArchetypeDefinition[] — strategic identity prose, one per group + "neutral"
  keywords.ts             KeywordDefinition[] — currently exactly guard + armored
  groups.ts               GroupDefinition[] — the four tribes (ironbound, emberkin, bloomtide, echowisp)
  visual-profiles.ts       VisualProfile[] — presentation-only data, one normal/forged pair per group
  cards/
    ironbound.ts, emberkin.ts, bloomtide.ts, echowisp.ts   8 collectible cards each
    neutral.ts                                              4 groupless collectible cards
    tokens.ts                                                non-collectible summon-only cards
```

The current roster (proven by `sim/content/content.test.ts`): **36
collectible cards** — 4 groups × 8 cards + 4 neutrals — plus 2 non-collectible
tokens (`bloom_sprout`, `bloom_seedling`).

## The real shapes

From `sim/content/schema.ts` (trimmed to the fields you author):

```ts
interface CardDefinition {
  readonly id: CardId;                    // stable string, e.g. "iron_recruit"
  readonly name: string;
  readonly rulesText: string;              // flavor/rules prose shown to players
  readonly tier: 1 | 2 | 3 | 4 | 5 | 6;    // gates shop availability by forge rank
  readonly cost: number;                   // shop gold cost
  readonly baseAttack: number;
  readonly baseHealth: number;             // must be >= 1
  readonly groups: readonly GroupId[];     // 0 or more tribe ids ([] = neutral)
  readonly keywords: readonly KeywordId[]; // "guard" / "armored" today
  readonly normal: readonly Ability[];     // abilities while unforged
  readonly forged: readonly Ability[];     // abilities once forged (replaces normal, not additive)
  readonly forgedStats: { attack: number; health: number };  // flat bonus applied on forge
  readonly visualProfile: VisualProfileId;
  readonly forgedVisualProfile: VisualProfileId;
  readonly poolCount: number;              // copies in the shared pool (0 for tokens)
  readonly tokens?: readonly TokenId[];    // token cardIds this card can summon (must be real cards)
  readonly collectible: boolean;           // false ⇒ never appears in a shop (tokens, forge-only cards)
  readonly forgeReward?: ForgeReward;      // overrides the default { kind: "gold", amount: 1 }
  readonly contentVersion: number;         // per-card version tag; authored but NOT currently
                                            // validated or read by the engine — a future migration hook
}

interface GroupDefinition {
  readonly id: GroupId;
  readonly name: string;
  readonly description: string;
  readonly archetype: ArchetypeId;         // must reference a real ArchetypeDefinition
  readonly visualTheme: string;
  readonly preferredTags: readonly string[];
  readonly shopWeight: number;             // advisory metadata only — the pool is the real source of what rolls
  readonly presentationCues: readonly string[];
  readonly accent: string;                 // hex UI accent color
}

interface VisualProfile {
  readonly id: VisualProfileId;
  readonly frame: string; readonly portrait: string; readonly border: string; readonly base: string;
  readonly idle: string; readonly entrance: string; readonly attackTrail: string; readonly impact: string;
  readonly death: string; readonly aura: string;
  readonly groupColor: string;             // hex
  readonly particleBudget: number;
  readonly soundCues: readonly string[];
  readonly forgedOverrides?: Partial<Omit<VisualProfile, "id" | "forgedOverrides">>;
}
```

All of these fields are asset/id **keys**, not asset data — the actual art,
audio, and models live entirely in the (not-yet-built) presentation layer.
This is deliberate: art can change without ever touching a card's rules, and
vice versa.

## Ability shape (the effect language)

```ts
interface Ability {
  readonly trigger: Trigger;                    // "on_buy" | "before_attack" | … (see EFFECT_LANGUAGE.md)
  readonly conditions?: readonly Condition[];    // AND-ed; all must hold or the ability does nothing
  readonly operations: readonly Operation[];     // executed in order when it fires
}
```

`CardDefinition.normal` and `.forged` are each an `Ability[]` — a card is
free to declare zero, one, or several abilities (up to
`EFFECT_BOUNDS.maxAbilitiesPerProfile = 6`), on the same or different
triggers. The forged profile **replaces** the normal profile entirely once a
unit forges (`source.forged ? def.forged : def.normal` — see `economy-
effects.ts`/`combat-effects.ts`); it does not layer on top of it, so a forged
card's abilities must be authored as the complete final behavior, typically a
strictly stronger version of the normal ability (bigger numbers, an extra
operation, a granted keyword) — every existing card in the roster follows
this "forged is a superset" convention, though it is not mechanically
enforced.

## Worked example: adding a full new group + unit + token + forged ability

This example adds everything requested: a new group (with a new archetype),
a new card in it with a token-summoning ability, a matching visual profile
pair, and a forged ability that differs in kind (not just numbers) from the
normal one.

### 1. Add the archetype (if the group needs a new strategic identity)

`sim/content/archetypes.ts`:

```ts
{
  id: "vigil",
  name: "Vigil",
  description: "Starforged identity: units that grow stronger the longer they survive a fight, and split into wardens when they finally fall.",
},
```

### 2. Add the group

`sim/content/groups.ts`:

```ts
{
  id: "starforged",
  name: "Starforged",
  description: "Celestial sentinels bound to hold the line until the very end, splintering into smaller wardens when they fall.",
  archetype: "vigil",           // must match the id from step 1
  visualTheme: "obsidian plate etched with drifting starlight",
  preferredTags: ["celestial", "sentinel"],
  shopWeight: 1,
  presentationCues: ["starlight flicker", "low chime on death-split"],
  accent: "#4C6FE0",
},
```

### 3. Add a visual profile pair

`sim/content/visual-profiles.ts` — one normal + one forged entry, following
the existing "forged is strictly richer" convention (bigger `particleBudget`,
more `soundCues`):

```ts
{
  id: "vp_starforged_normal",
  frame: "frame_starforged_etched", portrait: "portrait_starforged_sentinel",
  border: "border_starforged_silver", base: "base_starforged_plate",
  idle: "idle_starforged_still", entrance: "entrance_starforged_descend",
  attackTrail: "trail_starforged_arc", impact: "impact_starforged_chime",
  death: "death_starforged_splinter", aura: "aura_starforged_none",
  groupColor: "#4C6FE0", particleBudget: 14, soundCues: ["chime_low", "stone_grind"],
},
{
  id: "vp_starforged_forged",
  frame: "frame_starforged_radiant", portrait: "portrait_starforged_warden",
  border: "border_starforged_gold", base: "base_starforged_plate_glowing",
  idle: "idle_starforged_still_charged", entrance: "entrance_starforged_descend_bright",
  attackTrail: "trail_starforged_arc_starburst", impact: "impact_starforged_chime_deep",
  death: "death_starforged_supernova_splinter", aura: "aura_starforged_starlight",
  groupColor: "#4C6FE0", particleBudget: 26, soundCues: ["chime_high", "stone_grind_deep", "star_ring"],
},
```

### 4. Add the token this unit will summon on death

`sim/content/cards/tokens.ts` — tokens carry no abilities of their own (so
they can never form a summon cycle), never appear in a shop
(`collectible: false`, `poolCount: 0`), and never forge:

```ts
{
  id: "starforged_warden",
  name: "Starforged Warden",
  rulesText: "A splinter of a fallen sentinel, still bound to hold the line.",
  tier: 2, cost: 0, baseAttack: 2, baseHealth: 3,
  groups: ["starforged"], keywords: [],
  normal: [], forged: [],
  forgedStats: { attack: 0, health: 0 },
  visualProfile: "vp_starforged_normal", forgedVisualProfile: "vp_starforged_forged",
  poolCount: 0, collectible: false, contentVersion: 1,
},
```

### 5. Add the collectible card, referencing the token and both profile ids

`sim/content/cards/starforged.ts` (a new file — remember to add its export to
`ContentBundle.cards` in `bundle.ts`):

```ts
export const STARFORGED_CARDS: readonly CardDefinition[] = [
  {
    id: "star_sentinel",
    name: "Star Sentinel",
    rulesText: "Holds the line until it finally falls, splitting into a warden to carry on the vigil.",
    tier: 3, cost: 4, baseAttack: 4, baseHealth: 6,
    groups: ["starforged"], keywords: ["guard"],
    normal: [
      {
        trigger: "on_death",
        operations: [{ kind: "summon_token", token: "starforged_warden", at: { kind: "empty_friendly_slot" }, count: 1 }],
      },
    ],
    forged: [
      // The forged profile is not just bigger numbers here — it ADDS a whole
      // new behavior (self-healing on survive) on top of the death-split.
      {
        trigger: "on_death",
        operations: [{ kind: "summon_token", token: "starforged_warden", at: { kind: "empty_friendly_slot" }, count: 2 }],
      },
      {
        trigger: "on_survive_damage",
        operations: [{ kind: "heal", target: { kind: "self" }, amount: 1 }],
      },
    ],
    forgedStats: { attack: 2, health: 3 },
    tokens: ["starforged_warden"],       // MUST list every token this card can summon
    visualProfile: "vp_starforged_normal",
    forgedVisualProfile: "vp_starforged_forged",
    poolCount: 15,
    collectible: true,
    contentVersion: 1,
  },
  // … 7 more Starforged cards to match the established 8-per-group roster shape (optional, not enforced)
];
```

Then in `sim/content/bundle.ts`:

```ts
import { STARFORGED_CARDS } from "./cards/starforged.ts";
// …
cards: [...IRONBOUND_CARDS, ...EMBERKIN_CARDS, ...BLOOMTIDE_CARDS, ...ECHOWISP_CARDS,
        ...NEUTRAL_CARDS, ...STARFORGED_CARDS, ...TOKEN_CARDS],
```

That's it — no engine file changes. `Match`, `LoadedContent`, the pool, the
combat engine, and the interpreters all pick up the new group/cards/tokens
automatically the next time `loadDefaultContent()` runs, because everything
downstream reads from `LoadedContent`'s indexes, never from a hardcoded card
list.

## What the validator catches

`validateContent` (`sim/content/validate.ts`) runs at `LoadedContent`
construction and **throws** (blocking the match engine from ever seeing
broken content) if it returns any errors. `loadDefaultContent()` /
`new LoadedContent(bundle)` is where this happens — a broken bundle never
reaches `Match`. The exact error classes, verbatim:

| Mistake | Error text pattern |
|---|---|
| Duplicate id in any collection | `"<label>: duplicate id '<id>'"` (archetypes/keywords/groups/visualProfiles/cards) |
| Group references a nonexistent archetype | `"group '<id>': references missing archetype '<archetype>'"` |
| Card tier outside 1..6 | `"card '<id>': invalid tier <n> (expected 1..6)"` |
| Negative cost | `"card '<id>': negative cost <n>"` |
| Negative base attack | `"card '<id>': negative base attack <n>"` |
| Base health < 1 | `"card '<id>': base health <n> must be >= 1"` |
| Negative pool count | `"card '<id>': negative pool count <n>"` |
| Collectible card with `poolCount < 1` | `"card '<id>': collectible card must have pool count >= 1"` |
| Card references a nonexistent group | `"card '<id>': references missing group '<g>'"` |
| Card references a nonexistent keyword | `"card '<id>': references missing keyword '<k>'"` |
| Card's `visualProfile` doesn't exist | `"card '<id>': references missing visual profile '<id>'"` |
| Card's `forgedVisualProfile` doesn't exist | `"card '<id>': references missing forged visual profile '<id>'"` |
| Too many normal/forged abilities | `"card '<id>': too many normal abilities (<n>)"` (and `forged`) — cap `EFFECT_BOUNDS.maxAbilitiesPerProfile` (6) |
| `tokens` lists a nonexistent card | `"card '<id>': references missing token card '<t>'"` |
| Ability declares an unsupported trigger | `"<where>: unsupported trigger '<t>'"` |
| Ability declares an unsupported condition kind | `"<where>: unsupported condition '<k>'"` |
| Ability has zero operations | `"<where>: ability has no operations"` |
| Too many operations in one ability | `"<where>: <n> operations exceeds bound <max>"` — cap `EFFECT_BOUNDS.maxOperationsPerAbility` (8) |
| Operation has an unsupported kind | `"<where>: unsupported operation '<k>'"` |
| Operation used in the wrong context | `"<where>: operation '<k>' is not allowed in a <context> trigger"` — see `OPERATION_CONTEXT` in `EFFECT_LANGUAGE.md` |
| Operation's selector is unsupported | `"<where>: unsupported selector '<k>'"` |
| `summon_token` count out of bounds | `"<where>: summon count <n> exceeds bound 1..<max>"` — cap `EFFECT_BOUNDS.maxSummonPerOperation` (6) |
| `repeat` times out of bounds | `"<where>: repeat times <n> exceeds bound 1..<max>"` — cap `EFFECT_BOUNDS.maxRepeat` (8) |
| `repeat` nested too deep | `"<where>: repeat nesting exceeds depth <max>"` — cap `EFFECT_BOUNDS.maxOperationDepth` (3) |
| `summon_token.token` isn't a real card id | `"<where>: summons missing token '<t>'"` |
| `transform_card.into` isn't a real card id | `"<where>: transforms into missing card '<t>'"` |
| Bundle `version` isn't a positive integer | `"content: version must be a positive integer, got <n>"` |
| A card's tokens form a summon cycle (A summons B summons A) | `"content: recursive token summon cycle: A -> B -> A"` |

## Pool counts and canonical ordering

- `poolCount` is the number of copies of that card in the shared pool at
  match start (`initPool`, `sim/pool.ts`); the existing roster follows a
  tier-scaled convention (tier 1 = 18, tier 2 = 15, tier 3 = 13, tier 4 = 11,
  tier 5 = 9, tier 6 = 7) but this is convention, not a validated rule — any
  positive integer is legal for a collectible card.
- **Pool traversal is never object-key order.** `LoadedContent` (`content
  /load.ts`) sorts every collection by id at load time (`byId` helper for
  archetypes/keywords/groups/visualProfiles; cards by `(tier, id)`), and
  `drawFromPool` (`pool.ts`) always iterates `content.collectibleOfTier(tier)`
  in that canonical order, weighting by each card's remaining `poolCount`.
  This is what makes an identical seed roll an identical shop every time.
- `collectibleCards` = every card with `collectible: true` **and**
  `poolCount > 0` — a `collectible: true` card with `poolCount: 0` would be
  silently excluded from rolls (there is no validator error for this
  specific combination; only `collectible && poolCount < 1` is rejected,
  which is the same condition — so this case cannot actually occur in valid
  content).

## Content versioning

`ContentBundle.version` (top-level, currently `1` in `bundle.ts`'s `CONTENT`)
is the one version field the validator actually checks (`must be a positive
integer`). Each `CardDefinition` also carries its own `contentVersion` field
(currently `1` on every card) — this exists in the schema as a per-card
migration hook for the future, but nothing in the engine reads or validates
it today. Treat `ContentBundle.version` as the real content-schema version;
treat `contentVersion` as reserved for later per-card content migration.
