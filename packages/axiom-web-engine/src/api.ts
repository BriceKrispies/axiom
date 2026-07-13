/*
 * api.ts — the shared vocabulary of @axiom/web-engine. The whole package is pure
 * TypeScript over bare browser APIs (WebGL2, Canvas2D, requestAnimationFrame, DOM
 * events, WebAudio) — no wasm, no Rust-spine coupling. These shapes are the neutral
 * contract every consumer speaks: transforms, colors, mesh/material handles, lights,
 * a look-at camera, per-tick input, and procedural tones.
 *
 * The package is organized as a branchless, fully-tested spine (this contract, the
 * matrix math, mesh + shading generators, the retained-scene store, the fixed-step
 * accumulator, and the input state) behind a thin platform edge (the WebGL2 /
 * Canvas2D backends, the AudioContext synth, the requestAnimationFrame driver, the
 * DOM input binding, and the backend-constructing renderer facade).
 */

/** A scene-node id returned by `spawnRenderable` / `addLight`. */
export type Entity = number;

/** A mesh or material id. */
export type Handle = number;

/** An sRGB color, 0..1 components. */
export type Rgba = readonly [number, number, number, number];

/** A plain 3-vector. */
export interface EngineVec3 {
  readonly x: number;
  readonly y: number;
  readonly z: number;
}

/** A rotation quaternion as `[x, y, z, w]`. */
export type EngineQuat = readonly [number, number, number, number];

/** A node transform. Mesh conventions (unchanged from the previous renderer):
 * the `box` mesh is a UNIT CUBE (scale = full extents), the `sphere` mesh is
 * UNIT DIAMETER (scale = 2·radius), and the `cylinder` mesh is UNIT DIAMETER ×
 * UNIT HEIGHT around +Y (scale = (diameter, height, diameter)). */
export interface Transform {
  readonly position: EngineVec3;
  readonly rotation: EngineQuat;
  readonly scale: EngineVec3;
}

/** Custom triangle-list geometry passed to `createMeshData`. */
export interface MeshData {
  readonly positions: readonly EngineVec3[];
  readonly normals: readonly EngineVec3[];
  readonly indices: readonly number[];
}

export type MeshKind = "box" | "sphere" | "cylinder";

/** Lambert material: `baseColor` diffuse + additive `emissive`; `opacity` < 1
 * alpha-blends. `roughness` is accepted for vocabulary compatibility and has no
 * effect in this diffuse-only renderer. */
export interface MaterialSpec {
  readonly baseColor: Rgba;
  readonly emissive?: Rgba;
  readonly roughness?: number;
  readonly opacity?: number;
}

export type Light =
  | { readonly kind: "directional"; readonly direction: EngineVec3; readonly color: Rgba; readonly intensity: number }
  | { readonly kind: "point"; readonly position: EngineVec3; readonly color: Rgba; readonly intensity: number };

/** A look-at perspective camera; `fovY` is the vertical field of view in radians. */
export interface Camera3D {
  readonly position: EngineVec3;
  readonly target: EngineVec3;
  readonly fovY: number;
  readonly near: number;
  readonly far: number;
}

/** One tick's worth of input, read by a consumer during a fixed update. The DOM
 * edge accumulates events; `beginTick()` snapshots them so `pressed`/`released`
 * are exact one-tick edges and `look()` is the delta since the previous tick. */
export interface TickInput {
  readonly isDown: (action: string) => boolean;
  readonly pressed: (action: string) => boolean;
  readonly released: (action: string) => boolean;
  /** This tick's pointer-locked mouse delta (raw px, +x right / +y down). */
  readonly look: () => { readonly x: number; readonly y: number };
  /** The latest canvas pointer sample (CSS px, top-left origin), if any. */
  readonly pointer: () => { readonly pos: { readonly x: number; readonly y: number }; readonly down: boolean } | undefined;
}

/** A procedural tone (WebAudio oscillator + envelope). */
export interface ToneSpec {
  readonly wave: "sine" | "square" | "sawtooth" | "triangle";
  readonly freq: number;
  /** Seconds. */
  readonly duration: number;
  /** 0..1, default 0.15. */
  readonly volume?: number;
  /** Seconds from now to start (default 0) — lets one event play a two-note
   * figure without a scheduler. */
  readonly delay?: number;
}
