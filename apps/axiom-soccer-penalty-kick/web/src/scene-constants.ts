/*
 * The shared world geometry every subsystem measures against — the TS twin of the
 * constants `penalty_scene.rs` exports. One source of truth so the ball, goalie
 * volumes, result classifier, and scene builder all agree on where the goal, the
 * spot, and the ground are.
 *
 * Coordinate convention (right-handed): +X = right, +Y = up, +Z = toward the
 * shooter / away from the goal. The goal line is at z = 0; the penalty spot is at
 * z = 11. The ball flies from high +Z toward z = 0. Gravity is -Y.
 */

export const GROUND_Y = 0.0;
export const GOAL_HALF_WIDTH = 3.66;
export const GOAL_HEIGHT = 2.44;
export const POST_THICKNESS = 0.12;
export const GOAL_LINE_Z = 0.0;
export const PENALTY_SPOT_Z = 11.0;
export const GOALIE_X = 0.0;
export const GOALIE_Z = 0.5;
export const BALL_RADIUS = 0.32;

/** The net's rear plane sits this far behind the goal line (used by the goal-net panels + wobble). */
export const NET_DEPTH = 1.4;
