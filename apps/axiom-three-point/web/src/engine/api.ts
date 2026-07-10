/*
 * engine/api.ts — the shared vocabulary of the game's OWN engine. This app is
 * fully self-contained: everything under `web/src/engine/` is pure TypeScript
 * over bare browser APIs (WebGL2, requestAnimationFrame, DOM events, WebAudio) —
 * no `@axiom/game`, no wasm, no external packages. The shapes here deliberately
 * mirror the vocabulary the game code already speaks (`Transform`, `Rgba`,
 * mesh/material handles, a look-at camera), so the gameplay layer stays intact.
 *
 * The engine is split into four independent modules, all implementing the
 * contracts in this file:
 *   - `renderer.ts` — a WebGL2 forward renderer (meshes, Lambert materials,
 *     directional + point lights, look-at camera) behind the retained-scene
 *     functions (`createMesh` … `renderScene`).
 *   - `loop.ts`     — the deterministic fixed-step game loop (a testable
 *     accumulator core under a requestAnimationFrame driver).
 *   - `input.ts`    — keyboard actions, pointer-lock mouse look, and canvas
 *     pointer sampling (a testable state core under a DOM edge).
 *   - `audio.ts`    — a tiny WebAudio tone synth (`playTone`).
 */

/** A scene-node id returned by `spawnRenderable` / `addLight`. */
export type Entity = number;

/** A mesh or material id. */
export type Handle = number;

/** An sRGB color, 0..1 components. */
export type Rgba = readonly [number, number, number, number];

/** A plain 3-vector — structurally the game's own `vec.ts` `Vec3`. */
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

/** Custom triangle-list geometry (`meshgen.ts` produces this shape). */
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

/** One tick's worth of input, read by the game during a fixed update. The DOM
 * edge accumulates events; `beginTick()` snapshots them so `pressed`/`released`
 * are exact one-tick edges and `look()` is the delta since the previous tick. */
export interface TickInput {
  isDown(action: string): boolean;
  pressed(action: string): boolean;
  released(action: string): boolean;
  /** This tick's pointer-locked mouse delta (raw px, +x right / +y down). */
  look(): { readonly x: number; readonly y: number };
  /** The latest canvas pointer sample (CSS px, top-left origin), if any. */
  pointer(): { readonly pos: { readonly x: number; readonly y: number }; readonly down: boolean } | undefined;
}

/** A procedural tone (WebAudio oscillator + envelope). */
export interface ToneSpec {
  readonly wave: "sine" | "square" | "sawtooth" | "triangle";
  readonly freq: number;
  /** Seconds. */
  readonly duration: number;
  /** 0..1, default 0.15. */
  readonly volume?: number;
}
