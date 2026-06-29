/*
 * The retained game-object core (SPEC-14 §4.2 `this.add.*`). `Sim.add.sprite/
 * text/rectangle/image` each spawn an ECS entity carrying a `Transform` plus a
 * render component and return a `GameObject` HANDLE wrapping that entity. The
 * object is thin and bridge-backed: its mutators (`setPosition`/`setVelocity`/
 * `setRotation`/`setScale`) write the entity's components through the
 * `NativeBridge` world seam (the same `worldSpawn`/`worldSet` SPEC-02 exposes), so
 * a game object is just a typed cursor over native ECS state — never a second copy
 * of the world.
 *
 * The object keeps its own transform/velocity fields so a mutator writes the whole
 * component without a read-merge (which would need a branch on the get result);
 * the write-through keeps the native store authoritative for queries. Mutators
 * return the object for Phaser-style chaining.
 */

import type { Component, Entity, Vec2 } from "./vocabulary.ts";
import type { NativeBridge } from "./native-bridge.ts";

/** The default rotation / velocity component value. */
const ZERO = 0;
/** The default scale component value. */
const ONE = 1;

/** The explicit fields of a {@link TransformComponent}. */
interface TransformFields {
  readonly x: number;
  readonly y: number;
  readonly rotation: number;
  readonly scaleX: number;
  readonly scaleY: number;
}

/** A 2D transform component (position + rotation + scale) on a game object. */
interface TransformComponent extends Component {
  readonly kind: "Transform";
  readonly x: number;
  readonly y: number;
  readonly rotation: number;
  readonly scaleX: number;
  readonly scaleY: number;
}

/** A 2D linear-velocity component on a game object. */
interface VelocityComponent extends Component {
  readonly kind: "Velocity";
  readonly x: number;
  readonly y: number;
}

/** A textured-sprite render component. */
interface SpriteComponent extends Component {
  readonly kind: "Sprite";
  readonly texture: string;
}

/** A text render component. */
interface TextComponent extends Component {
  readonly kind: "Text";
  readonly value: string;
}

/** A filled-rectangle render component. */
interface RectangleComponent extends Component {
  readonly kind: "Rectangle";
  readonly width: number;
  readonly height: number;
  readonly color: number;
}

/** A static-image render component. */
interface ImageComponent extends Component {
  readonly kind: "Image";
  readonly texture: string;
}

/** The size + fill of a rectangle game object (SPEC-14 §4.2). */
export interface RectangleStyle {
  readonly width: number;
  readonly height: number;
  readonly color: number;
}

/** Build the `Transform` component from explicit fields. */
const transformComponent = (fields: TransformFields): TransformComponent => ({
  kind: "Transform",
  rotation: fields.rotation,
  scaleX: fields.scaleX,
  scaleY: fields.scaleY,
  x: fields.x,
  y: fields.y,
});

/** A retained handle wrapping one ECS entity and its render/transform components. */
export class GameObject {
  readonly #bridge: NativeBridge;
  readonly #entity: Entity;
  #x: number;
  #y: number;
  #rotation = ZERO;
  #scaleX = ONE;
  #scaleY = ONE;
  #vx = ZERO;
  #vy = ZERO;

  public constructor(bridge: NativeBridge, entity: Entity, origin: Vec2) {
    this.#bridge = bridge;
    this.#entity = entity;
    this.#x = origin.x;
    this.#y = origin.y;
  }

  /** The wrapped entity handle (so physics and queries can name it). */
  public get entity(): Entity {
    return this.#entity;
  }

  /** The object's x position. */
  public get x(): number {
    return this.#x;
  }

  /** The object's y position. */
  public get y(): number {
    return this.#y;
  }

  /** The object's rotation in radians. */
  public get rotation(): number {
    return this.#rotation;
  }

  /** The object's x scale. */
  public get scaleX(): number {
    return this.#scaleX;
  }

  /** The object's y scale. */
  public get scaleY(): number {
    return this.#scaleY;
  }

  /** The object's x velocity. */
  public get vx(): number {
    return this.#vx;
  }

  /** The object's y velocity. */
  public get vy(): number {
    return this.#vy;
  }

  /** Move the object to `(x, y)`, writing its `Transform`. */
  public setPosition(x: number, y: number): this {
    this.#x = x;
    this.#y = y;
    return this.#writeTransform();
  }

  /** Set the object's rotation, writing its `Transform`. */
  public setRotation(rotation: number): this {
    this.#rotation = rotation;
    return this.#writeTransform();
  }

  /** Set the object's scale, writing its `Transform`. */
  public setScale(scaleX: number, scaleY: number): this {
    this.#scaleX = scaleX;
    this.#scaleY = scaleY;
    return this.#writeTransform();
  }

  /** Set the object's linear velocity, writing its `Velocity` component. */
  public setVelocity(vx: number, vy: number): this {
    this.#vx = vx;
    this.#vy = vy;
    const velocity: VelocityComponent = { kind: "Velocity", x: vx, y: vy };
    this.#bridge.worldSet(this.#entity, velocity);
    return this;
  }

  /** Despawn the object's entity (it leaves the world). */
  public destroy(): void {
    this.#bridge.worldDespawn(this.#entity);
  }

  #writeTransform(): this {
    this.#bridge.worldSet(
      this.#entity,
      transformComponent({
        rotation: this.#rotation,
        scaleX: this.#scaleX,
        scaleY: this.#scaleY,
        x: this.#x,
        y: this.#y,
      }),
    );
    return this;
  }
}

/** Spawn an entity with a default `Transform` plus `render`, returning its handle. */
const spawnObject = (bridge: NativeBridge, render: Component, origin: Vec2): GameObject => {
  const entity = bridge.worldSpawn([
    transformComponent({ rotation: ZERO, scaleX: ONE, scaleY: ONE, x: origin.x, y: origin.y }),
    render,
  ]);
  return new GameObject(bridge, entity, origin);
};

/** Build a sprite render component. */
const spriteComponent = (texture: string): SpriteComponent => ({ kind: "Sprite", texture });
/** Build a text render component. */
const textComponent = (value: string): TextComponent => ({ kind: "Text", value });
/** Build an image render component. */
const imageComponent = (texture: string): ImageComponent => ({ kind: "Image", texture });
/** Build a rectangle render component from its style. */
const rectangleComponent = (style: RectangleStyle): RectangleComponent => ({
  color: style.color,
  height: style.height,
  kind: "Rectangle",
  width: style.width,
});

/** The `this.add.*` retained-object factory (SPEC-14 §4.2). */
export interface Add {
  /** Spawn a sprite from a texture key at `(x, y)`. */
  readonly sprite: (texture: string, x: number, y: number) => GameObject;
  /** Spawn a text object with `value` at `(x, y)`. */
  readonly text: (value: string, x: number, y: number) => GameObject;
  /** Spawn a filled rectangle of `style` at `(x, y)`. */
  readonly rectangle: (x: number, y: number, style: RectangleStyle) => GameObject;
  /** Spawn a static image from a texture key at `(x, y)`. */
  readonly image: (texture: string, x: number, y: number) => GameObject;
}

/** Build the `Add` factory over `bridge` (SPEC-14 §4.2). */
export const makeAdd = (bridge: NativeBridge): Add => ({
  image: (texture: string, x: number, y: number): GameObject =>
    spawnObject(bridge, imageComponent(texture), { x, y }),
  rectangle: (x: number, y: number, style: RectangleStyle): GameObject =>
    spawnObject(bridge, rectangleComponent(style), { x, y }),
  sprite: (texture: string, x: number, y: number): GameObject =>
    spawnObject(bridge, spriteComponent(texture), { x, y }),
  text: (value: string, x: number, y: number): GameObject =>
    spawnObject(bridge, textComponent(value), { x, y }),
});
