// A FAKE HostBridge that records every call, with scriptable return values, for
// the math / host-bridge / bindAction free-function tests. Kept in its own file
// so each fake is one class (max-classes-per-file).

import type { HostBridge, Outcome, SessionConfig } from "../src/host-binding.ts";
import type { Entity } from "../src/vocabulary.ts";

export class FakeHost implements HostBridge {
  public clampReturn = 0;
  public normalizeReturn = 0;
  public overlapReturn: readonly Entity[] = [];
  public config: SessionConfig = { params: {}, seed: 0n };
  public readyCount = 0;
  public clampCalls: (readonly [number, number, number])[] = [];
  public normalizeCalls: number[] = [];
  public overlapCalls: (readonly [number, number, number])[] = [];
  public bindings: (readonly [string, readonly string[]])[] = [];
  public outcomes: Outcome[] = [];
  public outcomeSets: Readonly<Record<number, Outcome>>[] = [];

  public clamp(value: number, low: number, high: number): number {
    this.clampCalls.push([value, low, high]);
    return this.clampReturn;
  }

  public normalizeAngle(angle: number): number {
    this.normalizeCalls.push(angle);
    return this.normalizeReturn;
  }

  public overlapCircle(centerX: number, centerY: number, radius: number): readonly Entity[] {
    this.overlapCalls.push([centerX, centerY, radius]);
    return this.overlapReturn;
  }

  public bindAction(action: string, keys: readonly string[]): void {
    this.bindings.push([action, keys]);
  }

  public getSessionConfig(): SessionConfig {
    return this.config;
  }

  public notifyReady(): void {
    this.readyCount += 1;
  }

  public reportOutcome(outcome: Outcome): void {
    this.outcomes.push(outcome);
  }

  public reportOutcomes(results: Readonly<Record<number, Outcome>>): void {
    this.outcomeSets.push(results);
  }
}
