/*
 * groups.ts — the four data-defined tribes. Each references its archetype
 * (`archetypes.ts`) and carries its own accent color and presentation cues;
 * mechanical identity lives entirely in the cards' abilities (`cards/*.ts`).
 */

import type { GroupDefinition } from "./schema.ts";

export const GROUPS: readonly GroupDefinition[] = [
  {
    id: "ironbound",
    name: "Ironbound",
    description:
      "Constructs cast in the Ironbound foundry, drilled to hold a line and reinforce whoever stands beside them.",
    archetype: "formation",
    visualTheme: "riveted steel and furnace-forged plate",
    preferredTags: ["construct", "foundry", "wall"],
    shopWeight: 1,
    presentationCues: ["clanking joints", "hydraulic hiss", "sparks on impact"],
    accent: "#7C8A99",
  },
  {
    id: "emberkin",
    name: "Emberkin",
    description:
      "Fire-blooded duelists who burn hotter with every exchange, trading their own flesh for a bigger swing.",
    archetype: "aggression",
    visualTheme: "cracked magma skin and guttering embers",
    preferredTags: ["fire", "duelist", "berserker"],
    shopWeight: 1,
    presentationCues: ["ember trail", "heat shimmer", "roaring flare on death"],
    accent: "#E2572B",
  },
  {
    id: "bloomtide",
    name: "Bloomtide",
    description:
      "A living grove of root-constructs that seeds sprouts across the field and thickens as the board fills.",
    archetype: "swarm",
    visualTheme: "bioluminescent bark and unfurling petals",
    preferredTags: ["plant", "swarm", "grove"],
    shopWeight: 1,
    presentationCues: ["petal burst", "root creak", "spore drift"],
    accent: "#3FAE64",
  },
  {
    id: "echowisp",
    name: "Echowisp",
    description:
      "Flickering illusion-weavers who reposition, mirror, and borrow tricks faster than the eye can track.",
    archetype: "trickery",
    visualTheme: "violet afterimages and refracted light",
    preferredTags: ["illusion", "spirit", "trickster"],
    shopWeight: 1,
    presentationCues: ["afterimage smear", "chime on swap", "fading blink-out"],
    accent: "#9B6BD1",
  },
];
