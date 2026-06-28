/*
 * The `Sim` handed to every fixed update and the `Frame` handed to every render
 * (SPEC-00 §4.2). `Sim` exposes no wall-clock accessor — elapsed simulated time
 * is `tick * dt`, constant per game. The `rng`/`input`/`world` members are typed
 * STUBS at M0: discriminated placeholders the later subsystem specs (SPEC-01 rng,
 * SPEC-05 input, SPEC-02 world) fill in, projecting each subsystem's §4.2 surface
 * into the namespace named here.
 */

/** Deterministic RNG surface — filled by SPEC-01. */
export interface RngStub {
  readonly subsystem: "rng";
}

/** Input surface — filled by SPEC-05. */
export interface InputStub {
  readonly subsystem: "input";
}

/** Retained ECS world surface — filled by SPEC-02. */
export interface WorldStub {
  readonly subsystem: "world";
}

/** The deterministic simulation view handed to a fixed update. */
export interface Sim {
  /** The monotonic fixed-tick index this update runs at. */
  readonly tick: number;
  /** The constant fixed timestep in seconds (`1 / fixedHz`). */
  readonly dt: number;
  readonly rng: RngStub;
  readonly input: InputStub;
  readonly world: WorldStub;
}

/** The presentation view handed to a render — interpolated with `alpha`. */
export interface Frame {
  /** The latest completed fixed tick this frame presents. */
  readonly tick: number;
}

/** One second expressed in seconds — the numerator of `dt = 1 second / fixedHz`. */
const ONE_SECOND_IN_SECONDS = 1;

/** Build the deterministic `Sim` for `tick` at a `fixedHz` cadence. */
export const makeSim = (fixedHz: number, tick: number): Sim => ({
  dt: ONE_SECOND_IN_SECONDS / fixedHz,
  input: { subsystem: "input" },
  rng: { subsystem: "rng" },
  tick,
  world: { subsystem: "world" },
});

/** Build the presentation `Frame` for the latest completed `tick`. */
export const makeFrame = (tick: number): Frame => ({ tick });
