/*
 * match.ts — the authoritative Match orchestrator and phase state machine. It is
 * the single source of truth: it owns the serializable `MatchState`, the match
 * `Rng`, the instance-id allocator, the event sink, the ghost store, and the
 * replayable command log. Every state change flows through here — commands during
 * `shop`, and the deterministic phase chain
 * `shop → combat_prepare → combat → combat_resolve → round_transition → shop`
 * (or `match_complete`). Combat is computed deterministically at
 * `combat_prepare`; the `combat` phase is pure playback of the event stream; the
 * `combat_resolve` step applies consequences. The simulation timer (ticks) and
 * the visual countdown are separate: the UI reads `phaseDeadlineTick`, but only
 * the match decides when a phase ends.
 */

import type { Command, CommandResult } from "./commands.ts";
import { REJECT } from "./commands.ts";
import type { LoadedContent } from "./content/load.ts";
import { runBoardEconomyTrigger, grantRoundGold } from "./effects/economy-effects.ts";
import type { EconomyEnv } from "./effects/economy-effects.ts";
import { applyShopCommand } from "./economy.ts";
import type { ShopEnv } from "./economy.ts";
import { EventSink } from "./events.ts";
import type { SimEvent } from "./events.ts";
import { InstanceIdAllocator } from "./ids.ts";
import type { PlayerId } from "./ids.ts";
import type { MatchState, Phase, PlayerState, WarbandSnapshot } from "./model.ts";
import { WARBAND_SLOTS } from "./model.ts";
import { deriveSeed, Rng } from "./rng.ts";
import { initPool, returnShop, rollShop } from "./pool.ts";
import { computePairings } from "./pairing.ts";
import type { GhostStore } from "./pairing.ts";
import { applyRoundResolution } from "./resolution.ts";
import type { RoundResult } from "./resolution.ts";
import { runCombat } from "./combat/engine.ts";
import type { CombatResult } from "./combat/engine.ts";
import { snapshotWarband } from "./combat/board.ts";
import { computeStage } from "./stage.ts";
import { DEFAULT_RULES, FIXED_HZ } from "./tuning.ts";
import type { Rules } from "./tuning.ts";

export interface MatchPlayerInit {
  readonly name: string;
  readonly isBot: boolean;
}

export interface MatchInit {
  readonly matchId: string;
  readonly seed: number;
  readonly content: LoadedContent;
  readonly rules?: Rules;
  readonly players: readonly MatchPlayerInit[];
}

/** One entry of the replayable command log. */
export interface LoggedCommand {
  readonly seq: number;
  readonly round: number;
  readonly tick: number;
  readonly playerId: PlayerId;
  readonly command: Command;
}

export interface PhaseTransition {
  readonly seq: number;
  readonly from: Phase;
  readonly to: Phase;
  readonly round: number;
  readonly tick: number;
}

const LOBBY_SIZE = 8;

export class Match {
  public readonly state: MatchState;
  public readonly content: LoadedContent;
  public readonly rules: Rules;
  private readonly rng: Rng;
  private readonly alloc: InstanceIdAllocator;
  private readonly events: EventSink;
  private readonly ghostStore: GhostStore;
  private readonly commandLog: LoggedCommand[] = [];
  private readonly transitions: PhaseTransition[] = [];
  private ghostUsedLastRound: PlayerId | null = null;
  private combatCounter = 0;
  private pendingResults: RoundResult[] = [];

  public constructor(init: MatchInit) {
    if (init.players.length !== LOBBY_SIZE) {
      throw new Error(`Arena Forge match needs exactly ${LOBBY_SIZE} players, got ${init.players.length}`);
    }
    this.content = init.content;
    this.rules = init.rules ?? DEFAULT_RULES;
    this.rng = new Rng(deriveSeed(init.seed, 0x4152454e)); // "AREN"
    this.alloc = new InstanceIdAllocator(1);
    this.events = new EventSink(0);
    this.ghostStore = { snapshots: new Map() };
    const players: PlayerState[] = init.players.map((p, id) => ({
      id,
      name: p.name,
      health: this.rules.startingHealth,
      gold: 0,
      forgeRank: this.rules.startingForgeRank,
      shop: [],
      shopFrozen: false,
      hand: [],
      warband: Array.from({ length: WARBAND_SLOTS }, () => null),
      eliminated: false,
      placement: 0,
      lastOpponent: null,
      opponentHistory: [],
      combatResult: null,
      presentationStage: "workshop",
      isBot: p.isBot,
    }));
    this.state = {
      matchId: init.matchId,
      seed: init.seed,
      round: 0,
      phase: "lobby",
      tick: 0,
      phaseDeadlineTick: 0,
      players,
      activeOrder: players.map((p) => p.id),
      pairings: [],
      ghostUsedLastRound: null,
      pool: initPool(this.content),
      commandSeq: 0,
      eventSeq: 0,
    };
    this.events.emit({ kind: "match_start", matchId: init.matchId, seed: init.seed });
    this.syncSeqs();
  }

  // ── public surface ────────────────────────────────────────────────────────
  public start(): void {
    if (this.state.phase === "lobby") {
      this.beginShop();
    }
  }

  public getEvents(): readonly SimEvent[] {
    return this.events.all();
  }

  public eventsSince(from: number): readonly SimEvent[] {
    return this.events.since(from);
  }

  public getCommandLog(): readonly LoggedCommand[] {
    return this.commandLog;
  }

  public getTransitions(): readonly PhaseTransition[] {
    return this.transitions;
  }

  public activeCount(): number {
    return this.state.players.filter((p) => !p.eliminated).length;
  }

  /** This round's computed combat results (populated during the `combat` phase,
   * for the presentation layer to play back). */
  public getRoundResults(): readonly RoundResult[] {
    return this.pendingResults;
  }

  /** Advance one authoritative tick; ends the current timed phase at its deadline. */
  public tick(): void {
    this.state.tick += 1;
    const timed = this.state.phase === "shop" || this.state.phase === "combat";
    if (timed && this.state.tick >= this.state.phaseDeadlineTick) {
      this.advancePhase();
    }
  }

  /** Force the current timed phase to end now (deterministic accelerated
   * advancement for tests / the dev harness — production uses `tick`). */
  public advancePhase(): void {
    if (this.state.phase === "shop") {
      this.enterCombat();
    } else if (this.state.phase === "combat") {
      this.resolveCombat();
    }
  }

  /** Submit one authenticated command. Only shop-phase commands from an active,
   * known player are accepted; everything else is a stable rejection. */
  public submit(playerId: PlayerId, command: Command): CommandResult {
    const player = this.state.players[playerId];
    if (player === undefined) {
      this.events.emit({ kind: "command_rejected", playerId, reason: REJECT.unknownPlayer, command: command.type });
      this.syncSeqs();
      return { ok: false, reason: REJECT.unknownPlayer };
    }
    if (this.state.phase !== "shop") {
      this.events.emit({ kind: "command_rejected", playerId, reason: REJECT.wrongPhase, command: command.type });
      this.syncSeqs();
      return { ok: false, reason: REJECT.wrongPhase };
    }
    if (player.eliminated) {
      this.events.emit({ kind: "command_rejected", playerId, reason: REJECT.notActive, command: command.type });
      this.syncSeqs();
      return { ok: false, reason: REJECT.notActive };
    }
    this.state.commandSeq += 1;
    this.commandLog.push({ seq: this.state.commandSeq, round: this.state.round, tick: this.state.tick, playerId, command });
    const result = applyShopCommand(this.shopEnv(), player, command);
    this.updateStage(player);
    this.syncSeqs();
    return result;
  }

  // ── internal machine ──────────────────────────────────────────────────────
  private syncSeqs(): void {
    this.state.eventSeq = this.events.nextSeq();
    this.state.ghostUsedLastRound = this.ghostUsedLastRound;
  }

  private economyEnv(): EconomyEnv {
    return { rules: this.rules, content: this.content, rng: this.rng, events: this.events, state: this.state };
  }

  private shopEnv(): ShopEnv {
    return { ...this.economyEnv(), allocate: () => this.alloc.allocate() };
  }

  private setPhase(to: Phase): void {
    const from = this.state.phase;
    this.state.phase = to;
    this.transitions.push({ seq: this.events.nextSeq(), from, to, round: this.state.round, tick: this.state.tick });
    this.events.emit({ kind: "phase_changed", from, to, round: this.state.round });
  }

  private updateStage(player: PlayerState): void {
    const stage = computeStage(this.rules, this.content, player);
    if (stage !== player.presentationStage) {
      player.presentationStage = stage;
      this.events.emit({ kind: "stage_changed", playerId: player.id, stage });
    }
  }

  private refreshShop(player: PlayerState): void {
    if (player.shopFrozen) {
      player.shopFrozen = false;
      return;
    }
    returnShop(this.state.pool, player.shop);
    player.shop = rollShop(this.state.pool, this.content, this.rules, player.forgeRank, this.rng, () => this.alloc.allocate());
  }

  private beginShop(): void {
    this.state.round += 1;
    this.setPhase("shop");
    for (const player of this.state.players) {
      if (player.eliminated) {
        continue;
      }
      grantRoundGold(this.economyEnv(), player);
      this.refreshShop(player);
      runBoardEconomyTrigger(this.economyEnv(), player, "shop_start");
      this.updateStage(player);
      this.events.emit({ kind: "shop_start", round: this.state.round, playerId: player.id });
    }
    this.state.phaseDeadlineTick = this.state.tick + this.rules.shopTimerSeconds * FIXED_HZ;
    this.syncSeqs();
  }

  private snapshotActive(): Map<PlayerId, WarbandSnapshot> {
    const map = new Map<PlayerId, WarbandSnapshot>();
    for (const player of this.state.players) {
      if (!player.eliminated) {
        map.set(player.id, snapshotWarband(player.id, player.forgeRank, player.warband, false));
      }
    }
    return map;
  }

  private byeResult(combatId: number, snap: WarbandSnapshot): CombatResult {
    const survivors = snap.slots.filter((u) => u !== null).length;
    const tierSum = snap.slots.reduce((sum, u) => sum + (u === null ? 0 : this.content.card(u.cardId).tier), 0);
    this.events.emit({ kind: "combat_begin", combatId, a: snap.ownerId, b: null, ghost: false });
    this.events.emit({ kind: "combat_end", combatId, verdict: "win", winner: "a", survivors });
    return {
      combatId,
      winnerSide: "a",
      aVerdict: "win",
      bVerdict: "draw",
      survivors,
      winnerForgeRank: snap.forgeRank,
      survivingTierSum: tierSum,
      bound: false,
    };
  }

  private enterCombat(): void {
    this.setPhase("combat_prepare");
    const snaps = this.snapshotActive();
    const { pairings, ghostChosen } = computePairings(this.state, this.content, this.ghostStore, this.ghostUsedLastRound);
    this.state.pairings = pairings;
    this.events.emit({ kind: "pairings_set", round: this.state.round, pairings });

    this.pendingResults = [];
    for (const pairing of pairings) {
      const combatId = this.combatCounter;
      this.combatCounter += 1;
      const snapA = snaps.get(pairing.a) as WarbandSnapshot;
      if (pairing.b === null && pairing.ghostOf === null) {
        this.pendingResults.push({ pairing, result: this.byeResult(combatId, snapA), snapA, snapB: snapA });
        continue;
      }
      const rawB =
        pairing.b !== null
          ? (snaps.get(pairing.b) as WarbandSnapshot)
          : (this.ghostStore.snapshots.get(pairing.ghostOf as PlayerId) as WarbandSnapshot);
      const snapB: WarbandSnapshot = { ...rawB, ghost: pairing.b === null };
      const result = runCombat(
        {
          rules: this.rules,
          content: this.content,
          events: this.events,
          combatId,
          seed: deriveSeed(this.state.seed, this.state.round, combatId),
          allocate: () => this.alloc.allocate(),
        },
        snapA,
        snapB,
      );
      this.pendingResults.push({ pairing, result, snapA, snapB });
    }

    this.ghostUsedLastRound = ghostChosen;
    this.setPhase("combat");
    this.state.phaseDeadlineTick = this.state.tick + this.rules.combatPlaybackSeconds * FIXED_HZ;
    this.syncSeqs();
  }

  private resolveCombat(): void {
    this.setPhase("combat_resolve");
    const { matchWinner } = applyRoundResolution(
      this.state,
      this.rules,
      this.content,
      this.events,
      this.ghostStore,
      this.pendingResults,
    );
    for (const player of this.state.players) {
      if (!player.eliminated) {
        this.updateStage(player);
      }
    }
    this.pendingResults = [];
    this.setPhase("round_transition");
    if (matchWinner !== null || this.activeCount() <= 1) {
      const winner = matchWinner ?? (this.state.players.find((p) => !p.eliminated)?.id ?? 0);
      this.events.emit({ kind: "match_won", playerId: winner });
      this.setPhase("match_complete");
      this.syncSeqs();
      return;
    }
    this.beginShop();
  }
}
