/*
 * presets.ts — THE configurable team roster for the Battle Simulator. This file is
 * pure DATA: a handful of ready-made warbands you can watch fight. It is the one
 * place to edit to add, remove, or retune a team — nothing else in the battle sim
 * hardcodes a lineup.
 *
 * ── How to edit ────────────────────────────────────────────────────────────────
 *   • A preset is up to seven units (WARBAND_SLOTS). Order matters: slot 0 is the
 *     LEFTMOST fighter and attacks/defends first, so put your front-liners first.
 *   • Each unit is a real card id (see src/sim/content/cards/*.ts) plus a `forged`
 *     flag. `forged: true` uses the card's forged stats + forged ability + forged
 *     look — a stronger, upgraded version of the unit.
 *   • `cardId`s are validated at load: presets.test.ts fails if any id is unknown
 *     or non-collectible, so a typo can never ship a broken team.
 *   • The enemy team is generated to ~match your team's POWER (see power.ts), so a
 *     3-unit elite squad and a 7-unit swarm both get a fair opponent.
 *   • `accent` is a hex color for the team's UI stripe (cosmetic only).
 *
 * Card ids you can use (tier in parens):
 *   Ironbound  — iron_recruit(1) iron_shieldling(1,guard) iron_wallwright(2,armored)
 *                iron_drillmaster(2) iron_bulwark(3,guard+armored) iron_vanguard(4)
 *                iron_colossus(5,armored) iron_bastion_prime(6,guard+armored)
 *   Emberkin   — ember_stoker(1) ember_hotblood(1) ember_duelist(2) ember_pyroclast(2)
 *                ember_warbrand(3) ember_ashbringer(4) ember_infernal_champion(5)
 *                ember_pyrarch_ascendant(6)
 *   Bloomtide  — bloom_seed_scout(1) bloom_pod_tender(1) bloom_vine_warden(2)
 *                bloom_thorned_guard(2,guard) bloom_canopy_shaper(3)
 *                bloom_root_matriarch(4) bloom_evergreen_colossus(5)
 *                bloom_worldroot_avatar(6)
 *   Echowisp   — echo_flicker_sprite(1) echo_mirror_initiate(1) echo_wisp_dancer(2)
 *                echo_veil_trickster(2) echo_duplicate_weaver(3) echo_riftwalker(4)
 *                echo_paradox_construct(5) echo_infinite_reflection(6)
 *   Neutral    — neutral_coinwright(1) neutral_bargain_scout(2)
 *                neutral_journeyman_smith(4) neutral_forgeheart_titan(6)
 */

/** One unit in a preset team: a real card id, optionally forged. */
export interface PresetUnit {
  readonly cardId: string;
  readonly forged: boolean;
}

/** A ready-made warband for the Battle Simulator. */
export interface TeamPreset {
  readonly id: string;
  readonly name: string;
  readonly subtitle: string;
  /** UI accent color (hex). Cosmetic only. */
  readonly accent: string;
  /** Up to seven units, slot 0 = leftmost / attacks first. */
  readonly units: readonly PresetUnit[];
}

const u = (cardId: string, forged = false): PresetUnit => ({ cardId, forged });

/**
 * The team catalog. Edit freely — add a `TeamPreset`, tweak a lineup, forge a unit.
 * Keep each `units` list to seven or fewer entries.
 */
export const TEAM_PRESETS: readonly TeamPreset[] = [
  {
    id: "ironbound_wall",
    name: "Ironbound Wall",
    subtitle: "Guards & armor — outlast everything",
    accent: "#7C8A99",
    units: [
      u("iron_shieldling"),
      u("iron_wallwright"),
      u("iron_bulwark", true),
      u("iron_vanguard"),
      u("iron_colossus", true),
      u("iron_bastion_prime"),
      u("iron_drillmaster"),
    ],
  },
  {
    id: "emberkin_blitz",
    name: "Emberkin Blitz",
    subtitle: "Glass cannons — win before they swing",
    accent: "#E2572B",
    units: [
      u("ember_stoker"),
      u("ember_duelist"),
      u("ember_warbrand"),
      u("ember_ashbringer"),
      u("ember_infernal_champion"),
      u("ember_pyrarch_ascendant", true),
      u("ember_hotblood"),
    ],
  },
  {
    id: "bloomtide_grove",
    name: "Bloomtide Grove",
    subtitle: "Rooted bruisers — heavy, healthy, patient",
    accent: "#3FAE64",
    units: [
      u("bloom_thorned_guard"),
      u("bloom_vine_warden"),
      u("bloom_canopy_shaper"),
      u("bloom_root_matriarch", true),
      u("bloom_evergreen_colossus"),
      u("bloom_worldroot_avatar"),
      u("bloom_seed_scout"),
    ],
  },
  {
    id: "echowisp_coven",
    name: "Echowisp Coven",
    subtitle: "Trickery & copies — never fight fair",
    accent: "#9B6BD1",
    units: [
      u("echo_mirror_initiate"),
      u("echo_wisp_dancer"),
      u("echo_duplicate_weaver"),
      u("echo_riftwalker", true),
      u("echo_paradox_construct"),
      u("echo_infinite_reflection"),
      u("echo_flicker_sprite"),
    ],
  },
  {
    id: "mixed_vanguard",
    name: "Mixed Vanguard",
    subtitle: "One of each tribe — a toolbox army",
    accent: "#C8933F",
    units: [
      u("iron_bulwark", true),
      u("ember_ashbringer"),
      u("bloom_root_matriarch"),
      u("echo_riftwalker"),
      u("neutral_journeyman_smith"),
      u("neutral_forgeheart_titan"),
      u("iron_shieldling"),
    ],
  },
  {
    id: "titan_trio",
    name: "Titan Trio",
    subtitle: "Three forged titans — quality over quantity",
    accent: "#F0C46A",
    units: [
      u("iron_bastion_prime", true),
      u("ember_pyrarch_ascendant", true),
      u("neutral_forgeheart_titan", true),
    ],
  },
];
