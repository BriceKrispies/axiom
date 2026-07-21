/*
 * local-host.ts — the in-process authoritative host for the local eight-player
 * mode (one human + seven bots). It owns the `Match`, implements the transport-
 * neutral `MatchApi`, and drives the seven bots through the SAME command surface
 * the human uses — so the local game exercises the exact command/event contracts
 * a future online match would. Bots take their turn deterministically the moment
 * a shop phase opens; the human plays over the shop timer; `tick`/`advancePhase`
 * pace the match (production ticks, tests force-advance). The class adds the
 * host-only controls the browser loop needs on top of the wire-shaped `MatchApi`.
 */

import type { CommandResult } from "../sim/commands.ts";
import type { LoadedContent } from "../sim/content/load.ts";
import type { PlayerId } from "../sim/ids.ts";
import type { MatchState } from "../sim/model.ts";
import { Match } from "../sim/match.ts";
import type { MatchPlayerInit } from "../sim/match.ts";
import type { Rules } from "../sim/tuning.ts";
import type { BotContext, BotPolicy, DecisionRecord } from "../bots/policy.ts";
import { policyForPlayer } from "../bots/policy.ts";
import { DEFAULT_POLICIES } from "../bots/policies.ts";
import { runBotTurn } from "../bots/driver.ts";
import type { CommandEnvelope, EventBatch } from "./envelopes.ts";
import type { MatchApi } from "./match-api.ts";

export interface LocalHostInit {
  readonly seed: number;
  readonly content: LoadedContent;
  readonly rules?: Rules;
  readonly humanName?: string;
  readonly botNames?: readonly string[];
  /** When true, player 0 is also bot-driven (used by the headless harness). */
  readonly allBots?: boolean;
}

const BOT_NAMES = ["Cinderwe", "Hollowpin", "Brasslume", "Quenchel", "Ferrovax", "Slagmire", "Emberlot"];

export class LocalMatchHost implements MatchApi {
  private readonly match: Match;
  private readonly botIds: PlayerId[];
  private readonly policies = new Map<PlayerId, BotPolicy>();
  private readonly decisionLog: DecisionRecord[] = [];
  private lastBotRound = -1;

  public constructor(init: LocalHostInit) {
    const names = init.botNames ?? BOT_NAMES;
    const players: MatchPlayerInit[] = [
      { name: init.humanName ?? "You", isBot: init.allBots ?? false },
      ...names.slice(0, 7).map((name) => ({ name, isBot: true })),
    ];
    this.match = new Match({ matchId: `local-${init.seed}`, seed: init.seed, content: init.content, ...(init.rules ? { rules: init.rules } : {}), players });
    // Bots are ids 1..7; with allBots the human slot (0) is bot-driven too.
    this.botIds = players.map((_p, id) => id).filter((id) => (init.allBots ?? false) || id !== 0);
    for (const id of this.botIds) {
      this.policies.set(id, policyForPlayer(DEFAULT_POLICIES, id));
    }
  }

  // ── MatchApi (transport-neutral wire surface) ───────────────────────────────
  public submit(env: CommandEnvelope): CommandResult {
    return this.match.submit(env.playerId, env.command);
  }

  public view(): MatchState {
    return this.match.state;
  }

  public eventsSince(cursor: number): EventBatch {
    return { events: this.match.eventsSince(cursor), cursor: this.match.state.eventSeq };
  }

  public isComplete(): boolean {
    return this.match.state.phase === "match_complete";
  }

  // ── host-only controls ──────────────────────────────────────────────────────
  public start(): void {
    this.match.start();
    this.runBotsIfNeeded();
  }

  public tick(): void {
    this.match.tick();
    this.runBotsIfNeeded();
  }

  public advancePhase(): void {
    this.match.advancePhase();
    this.runBotsIfNeeded();
  }

  /** Drive the whole match to completion (bots only pacing); for the harness. */
  public runToCompletion(): { readonly complete: boolean; readonly rounds: number } {
    this.start();
    const cap = this.match.rules.maxRounds * 4 + 8;
    let guard = 0;
    while (!this.isComplete() && guard < cap) {
      this.advancePhase();
      guard += 1;
    }
    return { complete: this.isComplete(), rounds: this.match.state.round };
  }

  public getMatch(): Match {
    return this.match;
  }

  public getDecisionLog(): readonly DecisionRecord[] {
    return this.decisionLog;
  }

  private botContext(): BotContext {
    return { state: this.match.state, content: this.match.content, rules: this.match.rules };
  }

  private runBotsIfNeeded(): void {
    if (this.match.state.phase !== "shop" || this.match.state.round === this.lastBotRound) {
      return;
    }
    this.lastBotRound = this.match.state.round;
    const deps = {
      snapshot: (): BotContext => this.botContext(),
      submit: (playerId: PlayerId, command: Parameters<Match["submit"]>[1]): CommandResult => this.match.submit(playerId, command),
    };
    for (const id of this.botIds) {
      if (!(this.match.state.players[id]?.eliminated ?? true)) {
        const policy = this.policies.get(id);
        if (policy !== undefined) {
          this.decisionLog.push(...runBotTurn(deps, id, policy));
        }
      }
    }
  }
}
