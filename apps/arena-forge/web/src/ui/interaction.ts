/*
 * interaction.ts — the mobile pointer controller. It turns taps, drags, and
 * tap-holds on the single canvas into AUTHORITATIVE COMMANDS submitted through
 * the match API — the UI never mutates state. Supported gestures: tap a card to
 * inspect, tap the contextual action to buy/play/sell, drag a shop card onto a
 * hand/warband destination to buy-and-place, drag a hand card onto a slot to
 * play, drag a warband unit onto another slot to reorder, drag a unit to the sell
 * zone to sell, tap reroll/freeze/upgrade, tap-hold for the full rules view, tap
 * outside to close. A cancelled or invalid drag submits nothing — no gold or card
 * is ever lost. Mouse uses the exact same pointer path.
 */

import type { Command } from "../sim/commands.ts";
import type { InstanceId, PlayerId } from "../sim/ids.ts";
import type { MatchState } from "../sim/model.ts";
import { inRect } from "./draw.ts";
import type { Layout } from "./layout.ts";
import { inspectRects } from "./layout.ts";

export type TargetKind = "shop" | "hand" | "warband" | "reroll" | "freeze" | "upgrade" | "sell" | "none";
export interface Target {
  readonly kind: TargetKind;
  readonly index: number;
}

export interface UiState {
  inspect: Target | null;
  detail: boolean;
  drag: { target: Target; x: number; y: number } | null;
}

const HOLD_MS = 420;
const DRAG_PX = 10;

const NONE: Target = { kind: "none", index: -1 };

export class Interaction {
  public readonly ui: UiState = { inspect: null, detail: false, drag: null };
  private layout: Layout | null = null;
  private state: MatchState | null = null;
  private downTarget: Target = NONE;
  private downX = 0;
  private downY = 0;
  private downAt = 0;
  private moved = false;
  private active = false;

  public constructor(
    private readonly humanId: PlayerId,
    private readonly submit: (cmd: Command) => void,
  ) {}

  /** Refresh the frame context the gestures hit-test against. */
  public setContext(layout: Layout, state: MatchState): void {
    this.layout = layout;
    this.state = state;
  }

  private hit(x: number, y: number): Target {
    const l = this.layout;
    if (l === null) {
      return NONE;
    }
    if (inRect(l.buttons.reroll, x, y)) return { kind: "reroll", index: 0 };
    if (inRect(l.buttons.freeze, x, y)) return { kind: "freeze", index: 0 };
    if (inRect(l.buttons.upgrade, x, y)) return { kind: "upgrade", index: 0 };
    if (inRect(l.sell, x, y)) return { kind: "sell", index: 0 };
    for (let i = 0; i < l.shop.length; i += 1) if (inRect(l.shop[i]!, x, y)) return { kind: "shop", index: i };
    for (let i = 0; i < l.hand.length; i += 1) if (inRect(l.hand[i]!, x, y)) return { kind: "hand", index: i };
    for (let i = 0; i < l.warband.length; i += 1) if (inRect(l.warband[i]!, x, y)) return { kind: "warband", index: i };
    return NONE;
  }

  private firstEmptySlot(): number {
    return this.state?.players[this.humanId]?.warband.findIndex((u) => u === null) ?? -1;
  }

  private warbandUnitAt(slot: number): InstanceId | null {
    return this.state?.players[this.humanId]?.warband[slot]?.instanceId ?? null;
  }

  private handUnitAt(i: number): InstanceId | null {
    return this.state?.players[this.humanId]?.hand[i]?.instanceId ?? null;
  }

  public onDown(x: number, y: number, now: number): void {
    // A tap inside an open inspect panel is handled on release; a tap outside closes it.
    this.active = true;
    this.downTarget = this.hit(x, y);
    this.downX = x;
    this.downY = y;
    this.downAt = now;
    this.moved = false;
  }

  public onMove(x: number, y: number, _now: number): void {
    if (!this.active) {
      return;
    }
    if (!this.moved && Math.hypot(x - this.downX, y - this.downY) > DRAG_PX) {
      this.moved = true;
      // Begin a drag only for draggable sources.
      if (this.downTarget.kind === "shop" || this.downTarget.kind === "hand" || this.downTarget.kind === "warband") {
        this.ui.drag = { target: this.downTarget, x, y };
        this.ui.inspect = null;
      }
    }
    if (this.ui.drag !== null) {
      this.ui.drag = { ...this.ui.drag, x, y };
    }
  }

  public onUp(x: number, y: number, _now: number): void {
    if (!this.active) {
      return;
    }
    this.active = false;
    const drag = this.ui.drag;
    this.ui.drag = null;
    if (drag !== null && this.moved) {
      this.resolveDrop(drag.target, x, y);
      return;
    }
    this.resolveTap(x, y);
  }

  /** Hold-to-inspect: detect a stationary long-press each frame. */
  public tick(now: number): void {
    if (this.active && !this.moved && this.ui.drag === null && now - this.downAt > HOLD_MS && this.ui.inspect === null) {
      if (this.downTarget.kind === "shop" || this.downTarget.kind === "hand" || this.downTarget.kind === "warband") {
        this.ui.inspect = this.downTarget;
        this.ui.detail = true;
      }
    }
  }

  private resolveDrop(source: Target, x: number, y: number): void {
    const dest = this.hit(x, y);
    if (dest.kind === "sell") {
      const id = source.kind === "warband" ? this.warbandUnitAt(source.index) : source.kind === "hand" ? this.handUnitAt(source.index) : null;
      if (id !== null) this.submit({ type: "sell", instanceId: id });
      return;
    }
    if (source.kind === "shop") {
      if (dest.kind === "warband" && this.state?.players[this.humanId]?.warband[dest.index] === null) {
        this.submit({ type: "buy", shopIndex: source.index, destination: { to: "warband", slot: dest.index } });
      } else {
        this.submit({ type: "buy", shopIndex: source.index, destination: { to: "hand" } });
      }
      return;
    }
    if (source.kind === "hand" && dest.kind === "warband") {
      const id = this.handUnitAt(source.index);
      if (id !== null) this.submit({ type: "play_card", instanceId: id, slot: dest.index });
      return;
    }
    if (source.kind === "warband" && dest.kind === "warband") {
      const id = this.warbandUnitAt(source.index);
      if (id !== null) this.submit({ type: "reorder", instanceId: id, slot: dest.index });
    }
  }

  private resolveTap(x: number, y: number): void {
    // Interactions with an open inspect panel take priority.
    if (this.ui.inspect !== null) {
      const rects = inspectRects(this.layout!.w, this.layout!.h);
      if (inRect(rects.close, x, y) || !inRect(rects.panel, x, y)) {
        this.ui.inspect = null;
        this.ui.detail = false;
        return;
      }
      if (inRect(rects.action, x, y)) {
        this.runInspectAction();
        this.ui.inspect = null;
        this.ui.detail = false;
        return;
      }
      return;
    }
    const t = this.hit(x, y);
    if (t.kind === "reroll") return this.submit({ type: "reroll" });
    if (t.kind === "upgrade") return this.submit({ type: "upgrade_forge_rank" });
    if (t.kind === "freeze") return this.submit({ type: "set_freeze", frozen: !(this.state?.players[this.humanId]?.shopFrozen ?? false) });
    if (t.kind === "shop" || t.kind === "hand" || t.kind === "warband") {
      this.ui.inspect = t;
      this.ui.detail = false;
    }
  }

  private runInspectAction(): void {
    const t = this.ui.inspect;
    if (t === null) {
      return;
    }
    if (t.kind === "shop") {
      const slot = this.firstEmptySlot();
      this.submit(slot >= 0 ? { type: "buy", shopIndex: t.index, destination: { to: "warband", slot } } : { type: "buy", shopIndex: t.index, destination: { to: "hand" } });
    } else if (t.kind === "hand") {
      const id = this.handUnitAt(t.index);
      const slot = this.firstEmptySlot();
      if (id !== null && slot >= 0) this.submit({ type: "play_card", instanceId: id, slot });
    } else if (t.kind === "warband") {
      const id = this.warbandUnitAt(t.index);
      if (id !== null) this.submit({ type: "sell", instanceId: id });
    }
  }
}
