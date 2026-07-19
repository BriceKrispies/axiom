/*
 * figure-pose.ts — the per-tick pose BUILDERS for the rigged figure: the running
 * gait (the one the whole feature exists for), the batting stance, and the fielder
 * idle. The running pose is a faithful port of the end-zone locomotion animator
 * (`presentation/locomotion/{gait,foot,pose}.rs` + `leg.rs`) with one deliberate
 * change: it is STATELESS. End-zone latches each foot's world contact in a
 * persistent `GaitState`; here the base runners travel a KNOWN deterministic path
 * at a known speed, so the same planted-foot anti-skate is reconstructed
 * closed-form from the runner's cumulative distance travelled (`traveled`) — the
 * stance foot's world lock is simply "where the hip was when this foot struck,
 * plus its reach". That keeps `view.ts` a pure function (it holds no gait state)
 * while producing the identical distance-driven, non-skating leg cycle.
 *
 * A "cycle" is one full two-step gait; the phase is `traveled / effectiveStride`,
 * so a full cycle covers `stride` world units of real travel. Tuning numbers are
 * the end-zone `LocomotionTuning` defaults.
 */

import { type Quat, type Vec3, IDENTITY_QUAT, add, quatFromEulerXyz, scale, sub, vec3 } from "./vec.ts";
import { transform, transformPoint } from "./figure-math.ts";
import {
  type JointPose,
  type LegDims,
  FIGURE_SCALE,
  HEAD,
  L_FOOT,
  L_FOREARM,
  L_HAND,
  L_SHIN,
  L_THIGH,
  L_UPPER_ARM,
  PARTS,
  PART_COUNT,
  PELVIS,
  R_FOOT,
  R_FOREARM,
  R_HAND,
  R_SHIN,
  R_THIGH,
  R_UPPER_ARM,
  TORSO,
  bodyTransform,
  legDims,
  neutralPose,
  solveLeg,
} from "./figure.ts";
import { quatInverse, quatMul } from "./figure-math.ts";

const TAU = Math.PI * 2;
const clamp = (v: number, lo: number, hi: number): number => Math.min(Math.max(v, lo), hi);
const lerp = (a: number, b: number, t: number): number => a + (b - a) * clamp(t, 0, 1);
const frac = (v: number): number => v - Math.floor(v);

/** The end-zone locomotion tuning defaults (yards / seconds / radians). Every
 * world-LENGTH knob is scaled by `FIGURE_SCALE` so the smaller figure takes
 * proportionally smaller steps; ratios, radians, and the speed threshold are
 * scale-free and stay as-is. */
const S = FIGURE_SCALE;
export const LOCOMOTION = {
  ankleGroundOffset: 0.09 * S,
  armSwing: 0.7,
  footLift: 0.34 * S,
  jogStride: 1.7 * S,
  landingDip: 0.05 * S,
  pelvisBob: 0.05 * S,
  pelvisYaw: 0.12,
  plantedFraction: 0.62,
  shoulderCounter: 0.16,
  sprintSpeed: 8.4,
  sprintStride: 3.2 * S,
  stanceHalfWidth: 0.14 * S,
  stanceReach: 0.22 * S,
  torsoLeanMax: 0.34,
} as const;

const qx = (a: number): Quat => quatFromEulerXyz(a, 0, 0);
const qy = (a: number): Quat => quatFromEulerXyz(0, a, 0);

/** A mutable pose under construction (the readonly `JointPose` is the frozen result). */
interface Building {
  joints: Quat[];
  rootLift: number;
  rootPitch: number;
  rootRoll: number;
}

const building = (): Building => ({ joints: Array.from({ length: PART_COUNT }, () => IDENTITY_QUAT), rootLift: 0, rootPitch: 0, rootRoll: 0 });
const freeze = (b: Building): JointPose => ({ joints: b.joints, rootLift: b.rootLift, rootPitch: b.rootPitch, rootRoll: b.rootRoll });

// ── running gait ─────────────────────────────────────────────────────────────────

/** Effective full-cycle stride from the normalized speed, bounded so cadence never
 * blurs — a stateless reduction of `gait.rs::effective_stride` (no startup / turn /
 * stopping ramps, since a base runner runs at a steady clip). */
const effectiveStride = (norm: number): number => clamp(lerp(LOCOMOTION.jogStride, LOCOMOTION.sprintStride, norm), LOCOMOTION.jogStride * 0.35, LOCOMOTION.sprintStride * 1.15);

/** Fraction of the cycle a foot stays planted — shrinks with stride so the
 * world-locked foot never over-extends the (stubby) leg (`foot.rs::planted_fraction`). */
const plantedFraction = (stride: number): number => Math.max(0.12, Math.min(LOCOMOTION.plantedFraction, (2 * LOCOMOTION.stanceReach) / Math.max(stride, 0.1)));

/** The compression dip near each foot strike (phase 0 and ½). */
const landingDip = (phase: number): number => {
  const d0 = Math.max(Math.cos(phase * TAU), 0);
  const d1 = Math.max(Math.cos((phase - 0.5) * TAU), 0);
  return Math.max(d0, d1) * LOCOMOTION.landingDip;
};

/** One foot's world ankle target this tick, reconstructed statelessly from the
 * runner's cumulative `traveled` distance. `lp` is the foot's local phase (0…1);
 * `latSign` is −1 (left) / +1 (right). Stance: the world lock is fixed under the
 * body (anti-skate); swing: an arc forward to the next reach-bounded landing. */
const footTarget = (ground: Vec3, forward: Vec3, rightDir: Vec3, stride: number, pf: number, lp: number, latSign: number): Vec3 => {
  const groundY = ground.y + LOCOMOTION.ankleGroundOffset;
  const lat = scale(rightDir, latSign * LOCOMOTION.stanceHalfWidth);
  // Where the hip was when THIS foot struck (lp = 0), plus the committed reach.
  const strike = sub(ground, scale(forward, lp * stride));
  const lock = add(add(strike, scale(forward, LOCOMOTION.stanceReach)), lat);
  if (lp < pf) {
    return vec3(lock.x, groundY, lock.z);
  }
  const landing = add(add(ground, scale(forward, LOCOMOTION.stanceReach)), lat);
  const s = clamp((lp - pf) / (1 - pf), 0, 1);
  const lift = Math.sin(s * Math.PI) * LOCOMOTION.footLift;
  return vec3(lerp(lock.x, landing.x, s), groundY + lift, lerp(lock.z, landing.z, s));
};

/** Solve both legs to their gait foot targets and level the feet (port of
 * `pose.rs::solve_legs` + `fl_level`). Mutates the leg joints in `b`. */
const solveLegs = (b: Building, ground: Vec3, facing: number, targetL: Vec3, targetR: Vec3): void => {
  const dims = legDims();
  const body = bodyTransform(ground, facing, freeze(b), 0);
  const pelvisLocal = transform(PARTS[PELVIS]!.offset, b.joints[PELVIS]!, vec3(1, 1, 1));
  const rParent = quatMul(body.rotation, b.joints[PELVIS]!);
  const forward = vec3(Math.sin(facing), 0, Math.cos(facing));
  const level = qy(facing);

  const hipL = transformPoint(body, transformPoint(pelvisLocal, PARTS[L_THIGH]!.offset));
  const hipR = transformPoint(body, transformPoint(pelvisLocal, PARTS[R_THIGH]!.offset));
  const left = solveLeg(dims, rParent, hipL, targetL, forward);
  const right = solveLeg(dims, rParent, hipR, targetR, forward);

  b.joints[L_THIGH] = left.thigh;
  b.joints[L_SHIN] = left.shin;
  b.joints[L_FOOT] = levelFoot(rParent, left.thigh, left.shin, level);
  b.joints[R_THIGH] = right.thigh;
  b.joints[R_SHIN] = right.shin;
  b.joints[R_FOOT] = levelFoot(rParent, right.thigh, right.shin, level);
};

/** The foot joint that levels the sole with the field (counter-rotate the
 * accumulated thigh+shin so the foot's world orientation is a plain yaw). */
const levelFoot = (rParent: Quat, thigh: Quat, shin: Quat, level: Quat): Quat => {
  const shinWorld = quatMul(quatMul(rParent, thigh), shin);
  return quatMul(quatInverse(shinWorld), level);
};

/**
 * The running pose for a figure at world `ground`, heading `facing`, moving at
 * `speed` (yd/s) having travelled `traveled` yd along its path. Stateless: the
 * whole distance-driven, planted-foot gait is a pure function of these.
 */
export const runningPose = (ground: Vec3, facing: number, speed: number, traveled: number): JointPose => {
  const b = building();
  const norm = clamp(speed / LOCOMOTION.sprintSpeed, 0, 1);
  const stride = effectiveStride(norm);
  const phase = frac(traveled / stride);
  const pf = plantedFraction(stride);
  const phaseAng = phase * TAU;
  const sw = Math.sin(phaseAng);
  const amp = 0.45 + 0.55 * norm;

  // A modest forward lean (kept at the end-zone value: a bigger lean pushes the
  // hip past the stubby leg's reach and the planted foot starts to skate).
  b.rootPitch = clamp(0.08 * norm, -0.18, LOCOMOTION.torsoLeanMax);
  b.rootRoll = 0;
  b.rootLift = Math.abs(Math.sin(phaseAng * 2)) * LOCOMOTION.pelvisBob - landingDip(phase);

  b.joints[PELVIS] = qy(LOCOMOTION.pelvisYaw * sw);
  b.joints[TORSO] = qy(-LOCOMOTION.shoulderCounter * sw);
  b.joints[HEAD] = qx(-b.rootPitch * 0.6);
  // Arms: upper-arm counter-swing to the legs, elbows bent (a runner's carry).
  b.joints[L_UPPER_ARM] = qx(-sw * LOCOMOTION.armSwing * amp);
  b.joints[R_UPPER_ARM] = qx(sw * LOCOMOTION.armSwing * amp);
  b.joints[L_FOREARM] = qx(-0.8);
  b.joints[R_FOREARM] = qx(-0.8);

  const forward = vec3(Math.sin(facing), 0, Math.cos(facing));
  const rightDir = vec3(Math.cos(facing), 0, -Math.sin(facing));
  const targetL = footTarget(ground, forward, rightDir, stride, pf, phase, -1);
  const targetR = footTarget(ground, forward, rightDir, stride, pf, frac(phase + 0.5), 1);
  solveLegs(b, ground, facing, targetL, targetR);
  return freeze(b);
};

// ── batting + idle override poses ─────────────────────────────────────────────────

/** A knees-bent athletic crouch (`k` = how deep), from `animation.rs::crouch`. */
const crouch = (b: Building, k: number): void => {
  b.joints[L_THIGH] = qx(-0.55 * k);
  b.joints[R_THIGH] = qx(-0.55 * k);
  b.joints[L_SHIN] = qx(0.9 * k);
  b.joints[R_SHIN] = qx(0.9 * k);
  b.joints[L_FOOT] = qx(-0.35 * k);
  b.joints[R_FOOT] = qx(-0.35 * k);
  b.rootPitch = 0.22 * k;
  b.rootLift = -0.16 * k;
};

/**
 * The batting stance: a coiled athletic crouch with the hands up by the shoulder,
 * the torso loading back while wound (`coil` 0…1) and unwinding through the swing
 * (`twist` 0…1, 1 = bat squared up). The visible bat (rendered separately) sells
 * the swing; this just puts the body and hands behind it.
 */
export const battingPose = (coil: number, twist: number): JointPose => {
  const b = building();
  crouch(b, 0.5);
  b.rootPitch = 0.16;
  // Load back while wound, rotate through square as the swing fires.
  const torsoYaw = -0.35 * coil + 0.7 * twist;
  b.joints[TORSO] = qy(torsoYaw);
  b.joints[PELVIS] = qy(0.35 * twist);
  // Hands up together by the trailing shoulder (the bat's knob lives near here).
  b.joints[R_UPPER_ARM] = quatFromEulerXyz(-1.35, 0, 0.35);
  b.joints[R_FOREARM] = qx(-1.1);
  b.joints[L_UPPER_ARM] = quatFromEulerXyz(-1.2, 0, -0.5);
  b.joints[L_FOREARM] = qx(-1.2);
  return freeze(b);
};

/** The arm segment lengths (upper arm = shoulder→elbow, forearm = elbow→wrist),
 * read from the model like `legDims` — reused through the two-bone solver. */
const armDims = (): LegDims => ({
  shin: Math.hypot(PARTS[L_FOREARM + 1]!.offset.x, PARTS[L_FOREARM + 1]!.offset.y, PARTS[L_FOREARM + 1]!.offset.z),
  thigh: Math.hypot(PARTS[L_UPPER_ARM + 1]!.offset.x, PARTS[L_UPPER_ARM + 1]!.offset.y, PARTS[L_UPPER_ARM + 1]!.offset.z),
});

/**
 * Solve BOTH arms so the hands reach a world `target` (the bat's grip), via the
 * same two-bone IK the legs use — so the batter actually grips the bat and the
 * hands track it through the swing. Returns a new pose with the six arm joints
 * overwritten; every other joint (legs, torso, root) is carried through unchanged.
 */
export const reachArmsTo = (pose: JointPose, ground: Vec3, facing: number, target: Vec3): JointPose => {
  const b: Building = { joints: [...pose.joints], rootLift: pose.rootLift, rootPitch: pose.rootPitch, rootRoll: pose.rootRoll };
  const body = bodyTransform(ground, facing, pose, 0);
  const pelvisLocal = transform(PARTS[PELVIS]!.offset, b.joints[PELVIS]!, vec3(1, 1, 1));
  const torsoLocal = transform(PARTS[TORSO]!.offset, b.joints[TORSO]!, vec3(1, 1, 1));
  // The upper arms hang from the TORSO frame — that is their solve parent.
  const torsoRot = quatMul(quatMul(body.rotation, b.joints[PELVIS]!), b.joints[TORSO]!);
  const dims = armDims();
  const elbowLead = vec3(0, -1, 0); // elbows drop toward the ground as the arms reach down

  const solveArm = (upper: number, fore: number, hand: number): void => {
    const shoulder = transformPoint(body, transformPoint(pelvisLocal, transformPoint(torsoLocal, PARTS[upper]!.offset)));
    const res = solveLeg(dims, torsoRot, shoulder, target, elbowLead);
    b.joints[upper] = res.thigh;
    b.joints[fore] = res.shin;
    b.joints[hand] = IDENTITY_QUAT;
  };
  solveArm(L_UPPER_ARM, L_FOREARM, L_HAND);
  solveArm(R_UPPER_ARM, R_FOREARM, R_HAND);
  return freeze(b);
};

/** The fielder idle: a shallow ready crouch, with an optional forward `lean`
 * (0…~0.2), an optional `headYaw` (the body stays squared up while ONLY the head +
 * its cap twist to watch), and a cheap `breath` signal (−1…1) that lifts the
 * shoulders and rocks the chest a touch so a standing player isn't a statue. The
 * breath only moves the upper body — never the root — so the FK feet stay planted. */
export const idlePose = (lean: number, headYaw = 0, breath = 0): JointPose => {
  const b = building();
  crouch(b, 0.3);
  b.rootPitch = 0.12 + lean;
  // Turn just the head to track the watched target; the torso does not rotate.
  b.joints[HEAD] = qy(headYaw);
  // Subtle breathing: the chest rocks a little and the shoulders lift on the
  // in-breath (a small upper-arm swing + a touch of arm-out roll) — the shoulders
  // rise and settle, feet unaffected.
  b.joints[TORSO] = qx(breath * 0.05);
  const shoulder = breath * 0.2;
  const spread = breath * 0.08;
  b.joints[L_UPPER_ARM] = quatFromEulerXyz(shoulder, 0, -0.18 - spread);
  b.joints[R_UPPER_ARM] = quatFromEulerXyz(shoulder, 0, 0.18 + spread);
  b.joints[L_FOREARM] = qx(-0.35 - lean);
  b.joints[R_FOREARM] = qx(-0.35 - lean);
  return freeze(b);
};

export { neutralPose };
export type { JointPose };
