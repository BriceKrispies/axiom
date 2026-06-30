/*
 * The client-side prediction core — the TypeScript twin of the Rust
 * `axiom-client-core` reconciliation path (`ClientCoreApi`'s pending queue +
 * `resimulate`).
 *
 * The server stays authoritative and the client applies snapshots newest-wins
 * (see `client.ts`); reconciliation owns the still-unacked local intents and the
 * one pure step prediction adds on top. After the client SNAPS to an
 * authoritative snapshot it RE-APPLIES its unacked intents over a caller-supplied
 * deterministic step, so the local player sees its own actions without waiting a
 * round-trip.
 *
 * `resimulate` is that step: a pure, game-state-generic fold of `step` over the
 * unacked intent payloads, in send order, from the just-snapped `baseline`. It is
 * sound only when the caller's fixed step is bit-identical between the authority
 * and this client (within one build that holds — the SPEC-13 §17.6 f32-determinism
 * prerequisite), so prediction is an explicit opt-in: with the flag off the
 * replayed count is `0` and `resimulate` is the identity, exactly as the Rust core
 * gates the count by `usize::from(flag)`.
 *
 * Branchless: the fold is `each` over a captured accumulator (no `reduce`, no
 * `for`), the gate is `Number(flag) * len`, and the queue edits are `filter`
 * combinators — the same shapes the byte-cursor decoders and the client use.
 */

import { each } from "./control-flow.ts";

const QUEUE_START = 0;

/** One still-unacknowledged local intent: its assigned sequence and the bytes sent. */
interface PendingIntent {
  readonly sequence: number;
  readonly payload: Uint8Array;
}

/** Fold `step` over the `unacked` intent payloads, in send order, starting from `baseline`. */
export const resimulate = <State>(
  baseline: State,
  unacked: readonly Uint8Array[],
  step: (state: State, payload: Uint8Array) => State,
): State => {
  const accumulator: { state: State } = { state: baseline };
  each(unacked, (payload): void => {
    accumulator.state = step(accumulator.state, payload);
  });
  return accumulator.state;
};

/**
 * The narrow prediction facade callers reach through `AxiomClient.predicting()`:
 * the opt-in toggle and the resimulation replay, WITHOUT the queue mutators the
 * client drives internally (record/ack/drop). The Rust `set_predict_local_player`
 * + `resimulate` surface.
 */
export interface Prediction {
  /** Opt in (or out) of local-player prediction. Default off (authoritative-only). */
  readonly setEnabled: (enabled: boolean) => void;
  /** Whether local-player prediction is enabled (`false` for a fresh client). */
  readonly enabled: () => boolean;
  /** Reconcile `baseline` by replaying the still-unacked local intents in send order. */
  readonly resimulate: <State>(baseline: State, step: (state: State, payload: Uint8Array) => State) => State;
}

/**
 * The unacked-intent queue and prediction toggle the {@link AxiomClient} delegates
 * to. It records each sent intent, drops them as the authority acks or rejects
 * them, and (when prediction is enabled) replays the survivors via {@link
 * resimulate}. The Rust `ClientCoreApi` queue + `set_predict_local_player` twin;
 * the client hands callers only its narrow {@link Prediction} facade.
 */
export class LocalPrediction implements Prediction {
  private enabledFlag = false;
  private pending: PendingIntent[] = [];

  /** Opt in (or out) of local-player prediction. Default off (authoritative-only). */
  public setEnabled(enabled: boolean): void {
    this.enabledFlag = enabled;
  }

  /** Whether local-player prediction is enabled (`false` for a fresh queue). */
  public enabled(): boolean {
    return this.enabledFlag;
  }

  /** Record a sent intent as pending, keyed by its monotonic client sequence. */
  public record(sequence: number, payload: Uint8Array): void {
    this.pending.push({ payload, sequence });
  }

  /** Drop every intent the authority has acknowledged (sequence `<=` `acked`). */
  public ackThrough(acked: number): void {
    this.pending = this.pending.filter((entry): boolean => entry.sequence > acked);
  }

  /** Drop one rejected intent from the queue. */
  public drop(sequence: number): void {
    this.pending = this.pending.filter((entry): boolean => entry.sequence !== sequence);
  }

  /** How many sent intents are still unacknowledged. */
  public count(): number {
    return this.pending.length;
  }

  /**
   * Reconcile `baseline` by replaying the still-unacked local intents in send
   * order. With prediction off the count is `0`, so this is the identity; the
   * count is the flag (`0`/`1`) times the queue length, so the disabled path
   * provably touches no intent.
   */
  public resimulate<State>(baseline: State, step: (state: State, payload: Uint8Array) => State): State {
    const count = Number(this.enabledFlag) * this.pending.length;
    return resimulate(baseline, this.pending.slice(QUEUE_START, count).map((entry): Uint8Array => entry.payload), step);
  }
}
