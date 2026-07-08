/*
 * Every tuning number for Signal Runner in one place — the "game data" the sim and
 * renderer specialize. Kept app-local and named so a future editor can read the
 * whole balance of the game at a glance.
 *
 * World units are abstract; the renderer projects them. Time is in seconds; the sim
 * runs at `FIXED_HZ`, so a "ticks" constant is `seconds * FIXED_HZ`.
 */

/** The fixed simulation rate (matches the manifest + boot). */
export const FIXED_HZ = 60;
/** The fixed timestep in seconds. */
export const DT = 1 / FIXED_HZ;

// ── Canvas ────────────────────────────────────────────────────────────────
export const WIDTH = 1200;
export const HEIGHT = 800;

// ── Route generation ────────────────────────────────────────────────────────
/** World units between two centerline nodes. */
export const SEG_LEN = 6;
/** How many nodes the route spans (≈ a 2–3 minute run at cruising speed). */
export const NODE_COUNT = 2600;
/** Base path half-width. */
export const PATH_HALF = 250;
/** Widened half-width at a pressure-plate node. */
export const PLATE_HALF = 360;
/** Distance the runner keeps clear at the very start (easy, wide, readable). */
export const INTRO_Z = 260;

/** Objective totals. */
export const SHARD_GOAL = 20;
export const PLATE_GOAL = 3;

// ── Movement ────────────────────────────────────────────────────────────────
/** Forward speed at the start of a run (world units / second). */
export const BASE_SPEED = 150;
/** Forward speed the runner ramps toward by the end of the route. */
export const MAX_SPEED = 320;
/** How fast forward speed eases toward its target (per second). */
export const SPEED_EASE = 1.6;
/** Extra forward speed while a boost is active. */
export const BOOST_SPEED = 170;
/** Forward-speed multiplier while braking. */
export const BRAKE_FACTOR = 0.55;
/** Forward-speed multiplier while off the path. */
export const OFFPATH_FACTOR = 0.4;

/** Lateral steer acceleration (world units / second²) at full input. */
export const STEER_ACCEL = 1500;
/** Steer-acceleration multiplier while braking (tighter turns). */
export const BRAKE_STEER_BONUS = 1.7;
/** Lateral velocity damping per second (0..1 fraction retained is `1 - this*DT`). */
export const LAT_DAMPING = 6;
/** How hard a curve pushes the runner outward (centrifugal drift). */
export const CURVE_DRIFT = 22;
/** Max lateral velocity magnitude. */
export const MAX_LAT_VEL = 420;
/** Analog drag steer gain toward the pointer target. */
export const DRAG_GAIN = 2400;

// ── Collection / collision tolerances ───────────────────────────────────────
/** Forward window for crossing a shard/plate/obstacle in one tick's travel. */
export const SHARD_LATERAL = 70;
export const PLATE_LATERAL = 220;
export const OBSTACLE_HIT = 46;
/** Charge gained per shard collected. */
export const SHARD_CHARGE = 0.09;
/** Lateral distance past the edge that counts as a fall (game over). */
export const FALL_MARGIN = 130;

// ── Crashes ─────────────────────────────────────────────────────────────────
export const MAX_CRASHES = 3;
/** Invulnerability window after a crash (seconds). */
export const INVULN_SECONDS = 1.4;
/** Forward speed retained after a crash. */
export const CRASH_SPEED_FACTOR = 0.35;
/** Lateral velocity impulse away from an obstacle on crash. */
export const CRASH_KNOCK = 160;

// ── Abilities ───────────────────────────────────────────────────────────────
export const BOOST_COST = 2 / 7;
export const SHIELD_COST = 2 / 7;
export const PULSE_COST = 2 / 7;
export const DRONE_COST = 3 / 7;
export const BOOST_SECONDS = 1.6;
export const SHIELD_SECONDS = 6;
export const BOOST_CD_SECONDS = 0.6;
export const SHIELD_CD_SECONDS = 1;
export const PULSE_CD_SECONDS = 1;
export const DRONE_CD_SECONDS = 1;
/** Pulse clears drones within this forward + lateral radius of the runner. */
export const PULSE_RADIUS_Z = 900;
export const PULSE_RADIUS_X = 400;
/** Helper-drone lifetime and how far ahead it ranges. */
export const HELPER_SECONDS = 5;
export const HELPER_SPEED = 520;
/** The number of charge segments the meter shows. */
export const CHARGE_SEGMENTS = 7;

// ── Storm / timer ───────────────────────────────────────────────────────────
/** Countdown the run starts with (2:30.0). */
export const STORM_SECONDS = 150;
/** Storm front speed at the start (world units / second). */
export const STORM_BASE_SPEED = 118;
/** Storm front speed near the end of the timer (it accelerates). */
export const STORM_MAX_SPEED = 240;
/** How far behind the runner the storm front begins. */
export const STORM_START_BACK = 1500;

// ── Beacon ──────────────────────────────────────────────────────────────────
/** Distance from the beacon within which activation is offered. */
export const ACTIVATE_Z = 260;

// ── Presentation mapping ────────────────────────────────────────────────────
/** World-speed → displayed KM/H factor (so cruising reads ≈ the reference's 68). */
export const KMH_FACTOR = 0.34;
