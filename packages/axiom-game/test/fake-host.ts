// A FAKE HostBridge that records every call, with scriptable return values, for
// the math / host-bridge / bindAction free-function tests. Kept in its own file
// so each fake is one class (max-classes-per-file).

import type {
  HostBridge,
  MusicOptions,
  Outcome,
  ScheduleOptions,
  SessionConfig,
  SoundOptions,
  ToneSpec,
} from "../src/host-binding.ts";
import type { Entity, Handle } from "../src/vocabulary.ts";

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

  // --- audio call log; voices/sounds get incrementing handles ---
  public loadedUrls: string[] = [];
  public playedSounds: (readonly [Handle, SoundOptions | undefined])[] = [];
  public stoppedVoices: Handle[] = [];
  public playedMusic: (readonly [readonly string[], MusicOptions | undefined])[] = [];
  public playedTones: ToneSpec[] = [];
  public scheduledSounds: (readonly [Handle, number, ScheduleOptions | undefined])[] = [];
  public masterVolumes: number[] = [];
  public muteStates: boolean[] = [];
  private nextHandle = 1;

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

  public loadSound(url: string): Handle {
    this.loadedUrls.push(url);
    return this.mint();
  }

  public playSound(id: Handle, opts?: SoundOptions): Handle {
    this.playedSounds.push([id, opts]);
    return this.mint();
  }

  public stopVoice(voice: Handle): void {
    this.stoppedVoices.push(voice);
  }

  public playMusic(urls: readonly string[], opts?: MusicOptions): Handle {
    this.playedMusic.push([urls, opts]);
    return this.mint();
  }

  public playTone(spec: ToneSpec): Handle {
    this.playedTones.push(spec);
    return this.mint();
  }

  public scheduleSound(id: Handle, atSeconds: number, opts?: ScheduleOptions): Handle {
    this.scheduledSounds.push([id, atSeconds, opts]);
    return this.mint();
  }

  public setMasterVolume(volume: number): void {
    this.masterVolumes.push(volume);
  }

  public setMuted(muted: boolean): void {
    this.muteStates.push(muted);
  }

  private mint(): Handle {
    const id = this.nextHandle;
    this.nextHandle += 1;
    return id;
  }
}
