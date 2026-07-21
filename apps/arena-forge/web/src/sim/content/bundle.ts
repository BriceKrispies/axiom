/*
 * bundle.ts — assembles every authored content file into one `ContentBundle`
 * and the `LoadedContent` the match engine consumes. This is the single
 * entry point content readers import; individual card/group/keyword/profile
 * files are implementation detail.
 */

import type { ContentBundle } from "./schema.ts";
import { LoadedContent } from "./load.ts";
import { ARCHETYPES } from "./archetypes.ts";
import { KEYWORDS } from "./keywords.ts";
import { GROUPS } from "./groups.ts";
import { VISUAL_PROFILES } from "./visual-profiles.ts";
import { IRONBOUND_CARDS } from "./cards/ironbound.ts";
import { EMBERKIN_CARDS } from "./cards/emberkin.ts";
import { BLOOMTIDE_CARDS } from "./cards/bloomtide.ts";
import { ECHOWISP_CARDS } from "./cards/echowisp.ts";
import { NEUTRAL_CARDS } from "./cards/neutral.ts";
import { TOKEN_CARDS } from "./cards/tokens.ts";

export const CONTENT: ContentBundle = {
  version: 1,
  archetypes: ARCHETYPES,
  keywords: KEYWORDS,
  groups: GROUPS,
  cards: [...IRONBOUND_CARDS, ...EMBERKIN_CARDS, ...BLOOMTIDE_CARDS, ...ECHOWISP_CARDS, ...NEUTRAL_CARDS, ...TOKEN_CARDS],
  visualProfiles: VISUAL_PROFILES,
};

export const loadDefaultContent = (): LoadedContent => new LoadedContent(CONTENT);
