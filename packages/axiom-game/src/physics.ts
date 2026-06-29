/*
 * The physics projection (SPEC-10 §4.2). `Sim.physics.add.{dynamic,static,
 * kinematic}` attaches a rigid body to a game object's entity and returns a
 * `Body` handle; the body's verbs (`applyImpulse`/`applyForce`/`applyTorque`,
 * velocity setters) and the world config (gravity + linear/angular damping) all
 * route to the native physics core through the `NativeBridge`. No integration math
 * lives in TS — the deterministic solver is native; this is a thin authoring skin.
 *
 * Damping is world config (per SPEC-10 §4.2 it is `PhysicsConfig`, not a per-call
 * verb), so `setConfig` is the one construction point for gravity + damping. The
 * `Body` is a closure over `(bridge, handle)` rather than a class so each verb is
 * a single forwarding call with no shared mutable state.
 */

import type { BodyKind, NativeBridge } from "./native-bridge.ts";
import type { Handle, Vec3 } from "./vocabulary.ts";
import type { GameObject } from "./game-object.ts";

/** The physics world configuration (SPEC-10 §4.2 `PhysicsConfig`). */
export interface PhysicsConfig {
  /** The constant gravity acceleration applied to every dynamic body. */
  readonly gravity: Vec3;
  /** The linear-velocity damping ratio in `[0, 1]` applied each step. */
  readonly linearDamping: number;
  /** The angular-velocity damping ratio in `[0, 1]` applied each step. */
  readonly angularDamping: number;
}

/** A rigid body attached to an entity (SPEC-10 §4.2). */
export interface Body {
  /** The native body handle. */
  readonly handle: Handle;
  /** Apply an instantaneous impulse (knockback). */
  readonly applyImpulse: (impulse: Vec3) => void;
  /** Apply a continuous force. */
  readonly applyForce: (force: Vec3) => void;
  /** Apply a torque (SPEC-10 angular). */
  readonly applyTorque: (torque: Vec3) => void;
  /** Set the body's linear velocity. */
  readonly setVelocity: (velocity: Vec3) => void;
  /** Set the body's angular velocity. */
  readonly setAngularVelocity: (velocity: Vec3) => void;
}

/** The `physics.add.*` body factory (SPEC-10 §4.2). */
export interface PhysicsAdd {
  /** Attach a dynamic (force-integrated) body to `object`. */
  readonly dynamic: (object: GameObject) => Body;
  /** Attach a static (immovable) body to `object`. */
  readonly static: (object: GameObject) => Body;
  /** Attach a kinematic (velocity-controlled) body to `object`. */
  readonly kinematic: (object: GameObject) => Body;
}

/** The physics factory on `Sim.physics` (SPEC-10 §4.2). */
export interface Physics {
  /** The body-attaching factory. */
  readonly add: PhysicsAdd;
  /** Set the physics world config (gravity + linear/angular damping). */
  readonly setConfig: (config: PhysicsConfig) => void;
}

/** Build a `Body` closure over `(bridge, handle)`. */
const makeBody = (bridge: NativeBridge, handle: Handle): Body => ({
  applyForce: (force: Vec3): void => {
    bridge.physicsApplyForce(handle, force);
  },
  applyImpulse: (impulse: Vec3): void => {
    bridge.physicsApplyImpulse(handle, impulse);
  },
  applyTorque: (torque: Vec3): void => {
    bridge.physicsApplyTorque(handle, torque);
  },
  handle,
  setAngularVelocity: (velocity: Vec3): void => {
    bridge.physicsSetAngularVelocity(handle, velocity);
  },
  setVelocity: (velocity: Vec3): void => {
    bridge.physicsSetVelocity(handle, velocity);
  },
});

/** Attach a `kind` body to `object`'s entity and wrap the native handle. */
const attachBody = (bridge: NativeBridge, object: GameObject, kind: BodyKind): Body =>
  makeBody(bridge, bridge.physicsAddBody(object.entity, kind));

/** Build the `Physics` projection over `bridge` (SPEC-10 §4.2). */
export const makePhysics = (bridge: NativeBridge): Physics => ({
  add: {
    dynamic: (object: GameObject): Body => attachBody(bridge, object, "dynamic"),
    kinematic: (object: GameObject): Body => attachBody(bridge, object, "kinematic"),
    static: (object: GameObject): Body => attachBody(bridge, object, "static"),
  },
  setConfig: (config: PhysicsConfig): void => {
    bridge.physicsSetConfig(config.gravity, config.linearDamping, config.angularDamping);
  },
});
