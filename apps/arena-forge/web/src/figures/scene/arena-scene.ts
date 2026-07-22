/*
 * arena-scene.ts — the ONE shared 3D scene the figures stand in: a forge floor, a
 * light rig, the look-at camera, and the world layout of the seven warband slots
 * (and the shop shelf behind them). It is rendered once per frame under the 2D UI
 * overlay. Everything here is presentation-only; the layout maps authoritative slot
 * indices to fixed world positions so figures are grounded and stable.
 */

import { addLight, createMaterial, createMesh, setCamera3D, setClearColor, spawnRenderable } from "@axiom/web-engine";
import type { Camera3D } from "@axiom/web-engine";
import { type Vec3, vec3 } from "../vec3.ts";
import type { ArenaStage } from "../../sim/model.ts";

const SLOT_SPACING = 1.18;
const WARBAND_Z = 0.35;
const SHOP_Z = -2.6;

export const warbandSlotPos = (i: number): Vec3 => vec3((i - 3) * SLOT_SPACING, 0, WARBAND_Z);
export const enemySlotPos = (i: number): Vec3 => vec3((i - 3) * SLOT_SPACING, 0, SHOP_Z + 0.4);
export const shopSlotPos = (i: number, count: number): Vec3 => vec3((i - (count - 1) / 2) * 1.0, 0.55, SHOP_Z);

const STAGE_FLOOR: Readonly<Record<ArenaStage, [number, number, number]>> = {
  workshop: [0.09, 0.075, 0.06],
  kindled: [0.11, 0.08, 0.055],
  tempered: [0.13, 0.085, 0.05],
  masterwork: [0.16, 0.095, 0.05],
};

const STAGE_GLOW: Readonly<Record<ArenaStage, number>> = { workshop: 0.5, kindled: 0.8, tempered: 1.2, masterwork: 1.8 };

/** The default combat-arena camera, framing the warband row. */
export const arenaCamera = (): Camera3D => ({ position: vec3(0, 2.7, 4.8), target: vec3(0, 0.85, 0), fovY: 0.72, near: 0.1, far: 80 });

/**
 * Build the GALLERY stage: the same light rig, but no floor and no ledge — the
 * Figure Lab gallery lays many miniatures out on a flat plane facing the camera,
 * so any ground geometry would slice through the grid. The camera is owned by the
 * caller (it derives it from the gallery's screen rect), not set here.
 */
export const buildGalleryStage = (): void => {
  setClearColor([0.028, 0.023, 0.018, 1]);
  // Key from the upper left. The gallery camera looks down −Z, so the faces the
  // camera actually sees carry +Z normals — and a fill/rim that travels toward +Z
  // lights the tops and backs but never the camera-facing front, leaving it a flat
  // dark slab lit only by the grazing key. So the cool fill is aimed camera-side
  // (−Z travel, from the right) to lift and model the front planes, while the warm
  // rim and underlight stay well below the key so the shadow side still falls dark.
  // Exposure matters as much as direction here: at intensity 2.5 the key drove the
  // camera-facing +Z fronts and the +Y tops past 1.0, where they clipped flat to
  // white — erasing both the plate colours and the form gradient, the exact washed,
  // even look the champion reads. Pulling the key back to ~1.2 (and trimming the
  // camera-side fill) seats the lit planes just under clip, so the top plates still
  // catch a near-white highlight while the fronts hold their steel/bronze/olive and
  // the shadow-side plates and plate-to-plate recesses fall dark — the punchy
  // chiaroscuro the reference reads, not a blown, uniform wash.
  addLight({ kind: "directional", direction: vec3(-0.4, -0.75, -0.52), color: [1, 0.96, 0.88, 1], intensity: 1.2 });
  addLight({ kind: "directional", direction: vec3(0.5, -0.28, -0.72), color: [0.6, 0.72, 0.98, 1], intensity: 0.4 });
  addLight({ kind: "directional", direction: vec3(0, 0.55, 1), color: [1, 0.62, 0.34, 1], intensity: 0.38 });
  addLight({ kind: "directional", direction: vec3(0, -1, 0.15), color: [0.75, 0.7, 0.8, 1], intensity: 0.1 });
};

/** Build the static arena (floor + lights + camera + clear color). Idempotent per
 * `clearScene`; call once after `initRenderer`. */
export const buildArena = (stage: ArenaStage): void => {
  setClearColor([0.05, 0.045, 0.05, 1]);
  setCamera3D(arenaCamera());

  const box = createMesh("box");
  const floorMat = createMaterial({ baseColor: [...STAGE_FLOOR[stage], 1] as [number, number, number, number] });
  spawnRenderable(box, floorMat, { position: vec3(0, -0.09, -1), rotation: [0, 0, 0, 1], scale: vec3(16, 0.18, 9) });
  // A raised, lighter ledge under the warband row for figure contrast.
  const ledgeMat = createMaterial({ baseColor: [0.26, 0.22, 0.19, 1] });
  spawnRenderable(box, ledgeMat, { position: vec3(0, -0.02, WARBAND_Z), rotation: [0, 0, 0, 1], scale: vec3(9.5, 0.14, 1.5) });
  spawnRenderable(box, ledgeMat, { position: vec3(0, 0.35, SHOP_Z), rotation: [0, 0, 0, 1], scale: vec3(8, 0.1, 1.1) });

  // Strong key + cool fill + a warm forge rim, so even dark-metal figures read.
  addLight({ kind: "directional", direction: vec3(-0.35, -0.9, -0.5), color: [1, 0.95, 0.86, 1], intensity: 1.9 });
  addLight({ kind: "directional", direction: vec3(0.6, -0.35, 0.55), color: [0.62, 0.74, 0.98, 1], intensity: 0.85 });
  addLight({ kind: "directional", direction: vec3(0, 0.2, 1), color: [1, 0.6, 0.35, 1], intensity: 0.7 });
  addLight({ kind: "point", position: vec3(0, 1, 2.4), color: [1, 0.62, 0.3, 1], intensity: 0.6 + STAGE_GLOW[stage] * 0.5 });
};
