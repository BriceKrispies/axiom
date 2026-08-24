/**
 * BOOT WEIGHTS — generated, do not hand-edit.
 *
 * How long each boot phase takes, measured, so the loading bar can pace
 * itself by real cost instead of counting steps. Regenerate after any change
 * that moves boot around:
 *
 *   node tools/bootprofile.mjs --emit-weights
 *
 * Captured on: ANGLE (NVIDIA Corporation, NVIDIA GeForce GTX 1080 Ti/PCIe/SSE2, OpenGL 4.5.0)
 * Regime: warm (a reload; a first visit is several times this,
 * which BootProgress discovers at runtime and re-prices — see bootprogress.js)
 */

/** Wall ms per phase. These partition the boot, so they sum to the total. */
export const BOOT_WEIGHTS = {
  "bakery.ready": 20,
  "engine.prepare": 1,
  "init:render": 276,
  "init:materials": 40,
  "init:sky": 35,
  "init:physics": 1,
  "init:world": 931,
  "init:player": 4,
  "init:weapons": 153,
  "init:fx": 22,
  "init:ai": 2,
  "init:ui": 34,
  "init:audio": 0,
  "engine.collectStream": 0,
  "engine.attach": 11,
  "prewarm.scene": 192,
  "boot-frames": 439
};

/** Sub-phases, used to move the bar THROUGH a long phase rather than at its end. */
export const BOOT_CHILDREN = {
  "init:world": {
    "world:registerProps": 65,
    "world:ground": 41,
    "world:building0": 38,
    "world:building1": 25,
    "world:building2": 51,
    "world:building3": 41,
    "world:building4": 28,
    "world:building5": 17,
    "world:building6": 55,
    "world:building7": 20,
    "world:building8": 31,
    "world:building9": 38,
    "world:building10": 14,
    "world:building11": 14,
    "world:building12": 11,
    "world:building13": 14,
    "world:building14": 8,
    "world:building15": 41,
    "world:building16": 26,
    "world:building17": 21,
    "world:building18": 13,
    "world:building19": 21,
    "world:gate": 8,
    "world:perimeter": 24,
    "world:dressStreet": 44,
    "world:dressBuildings": 45,
    "world:debris": 4,
    "world:lights": 2,
    "world:finalize": 170
  }
};

/** Sum of BOOT_WEIGHTS, in reference-machine milliseconds. */
export const BOOT_TOTAL_MS = 2161;
