// Ambient module declarations for the Strudel packages, which ship no TypeScript
// types. The tool only touches a tiny, stable slice of their surface; typing it
// as `any` keeps `tsgo --noEmit` clean without pretending to model the library.
// The real contract is pinned by test/exporter.test.ts against the exact
// package versions.

declare module '@strudel/transpiler' {
  export function transpiler(code: string, options?: Record<string, unknown>): { output: string };
}

declare module '@strudel/core' {
  export function evaluate(
    code: string,
    transpiler: unknown,
    options?: unknown,
  ): Promise<{ pattern: unknown; meta?: unknown }>;
  export function evalScope(...modules: Array<Promise<unknown>>): Promise<unknown>;
}

declare module '@strudel/mini';
declare module '@strudel/tonal';
declare module '@strudel/webaudio';

declare module 'superdough' {
  export function initAudio(options?: Record<string, unknown>): Promise<void>;
  export function registerSynthSounds(): void;
  export function superdough(
    value: unknown,
    time: number,
    duration: number,
    cps?: number,
    cycle?: number,
  ): Promise<void>;
  export function setAudioContext(ctx: unknown): unknown;
}
