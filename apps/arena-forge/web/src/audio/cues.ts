/*
 * cues.ts — maps the simulation's event stream to procedural audio via the Axiom
 * engine's `playTone`. Audio is pure presentation: it reads events and never
 * feeds back into the sim, so muting or dropping cues cannot change a result.
 * Repeated combat sounds (summon/damage chains) are throttled by a per-drain
 * budget so a big turn cannot become an unusable audio burst.
 */

import { playTone } from "@axiom/web-engine";
import type { SimEvent } from "../sim/events.ts";
import type { PlayerId } from "../sim/ids.ts";

interface Cue {
  readonly wave: "sine" | "square" | "sawtooth" | "triangle";
  readonly freq: number;
  readonly duration: number;
  readonly volume: number;
}

const CUES: Partial<Record<SimEvent["kind"], Cue>> = {
  card_purchased: { wave: "triangle", freq: 520, duration: 0.08, volume: 0.18 },
  command_rejected: { wave: "square", freq: 130, duration: 0.1, volume: 0.14 },
  card_sold: { wave: "sine", freq: 300, duration: 0.09, volume: 0.14 },
  shop_rerolled: { wave: "triangle", freq: 380, duration: 0.07, volume: 0.14 },
  shop_freeze_changed: { wave: "sine", freq: 660, duration: 0.09, volume: 0.14 },
  forge_rank_increased: { wave: "sawtooth", freq: 300, duration: 0.22, volume: 0.2 },
  card_played: { wave: "triangle", freq: 440, duration: 0.07, volume: 0.16 },
  unit_forged: { wave: "sawtooth", freq: 240, duration: 0.3, volume: 0.24 },
  combat_begin: { wave: "sawtooth", freq: 160, duration: 0.28, volume: 0.2 },
  attack_started: { wave: "square", freq: 220, duration: 0.05, volume: 0.1 },
  impact: { wave: "square", freq: 150, duration: 0.09, volume: 0.16 },
  unit_died: { wave: "sawtooth", freq: 110, duration: 0.14, volume: 0.16 },
  player_damaged: { wave: "square", freq: 90, duration: 0.16, volume: 0.2 },
  player_eliminated: { wave: "sawtooth", freq: 80, duration: 0.4, volume: 0.24 },
  match_won: { wave: "triangle", freq: 660, duration: 0.5, volume: 0.28 },
};

const MAX_TONES_PER_DRAIN = 5;

export class AudioCues {
  public masterVolume = 1;
  public effectsVolume = 1;
  public enabled = true;

  /** Play cues for the events relevant to the human this drain, throttled. */
  public play(events: readonly SimEvent[], humanId: PlayerId): void {
    if (!this.enabled) {
      return;
    }
    let budget = MAX_TONES_PER_DRAIN;
    for (const ev of events) {
      if (budget <= 0) {
        return;
      }
      // Only surface events about the human's own board / combat.
      if ("playerId" in ev && ev.playerId !== humanId) {
        continue;
      }
      const cue = CUES[ev.kind];
      if (cue !== undefined) {
        const vol = cue.volume * this.masterVolume * this.effectsVolume;
        playTone({ wave: cue.wave, freq: cue.freq, duration: cue.duration, volume: Math.max(0, Math.min(0.4, vol)) });
        budget -= 1;
      }
    }
  }
}
