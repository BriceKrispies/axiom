/*
 * solid.ts — LAYER 1 of the CSS3D treasure-chest build: the primitive layer.
 *
 * The only thing this file knows is how to turn a rectangular solid into DOM
 * elements placed by CSS 3D transforms. There is no canvas, no rasterizer, and
 * no 2D drawing of any kind: every visible surface is a real `<i>` element with
 * a `background`, positioned by `transform: translate3d(...) rotateX/Y(...)`
 * inside a `transform-style: preserve-3d` tree. The browser's compositor does
 * the projection, the perspective divide and the depth sorting.
 *
 * COORDINATES. The natural CSS element is an XY rectangle, so the ground plane
 * is XY and height is Z — the classic CSS-3D map convention, which lets every
 * flat thing (lagoon, shadows, sand) be an unrotated element:
 *
 *     +x → right        +y → toward the camera (near)
 *     -y → away (far)   +z → up
 *
 * The world root carries `rotateX(58deg)`, which tips that map into a 3/4 view:
 * +y then reads as down-screen-and-nearer, +z as up-screen. See `stage.css`.
 *
 * FACE RECIPES. For a solid spanning [x, x+w] × [y, y+d] × [z, z+h], with
 * `transform-origin: 0 0` on every face:
 *
 *     top    translate3d(x,     y+0, z+h)
 *     near   translate3d(x,     y+d, z+h) rotateX(-90deg)              (normal +y)
 *     left   translate3d(x,     y+0, z+h) rotateX(-90deg) rotateY(-90deg)
 *     right  translate3d(x+w,   y+d, z+h) rotateX(-90deg) rotateY(90deg)
 *
 * Only these four are emitted. The far face and the underside can never be seen
 * by a fixed 3/4 camera, and every element we do not create is compositing work
 * the browser never does — the entire performance budget of a DOM renderer.
 */

/** A solid's four painted surfaces, darkest-to-lightest as the light rakes it. */
export interface SolidPaint {
  /** The up-facing surface — catches the key light, so the lightest value. */
  readonly top: string;
  /** The camera-facing surface — the mid value that reads as the object's hue. */
  readonly near: string;
  /** The two side surfaces — the shadow value that carves the silhouette. */
  readonly side: string;
}

/** A box in world units: origin corner + extents. */
export interface SolidBox {
  readonly x: number;
  readonly y: number;
  readonly z: number;
  readonly w: number;
  readonly d: number;
  readonly h: number;
}

/** Which faces to emit. Dropping unseen faces is the main cost lever. */
export interface SolidOptions {
  readonly top?: boolean;
  readonly near?: boolean;
  readonly left?: boolean;
  readonly right?: boolean;
  /** Extra transform applied AFTER placement (e.g. a lid's hinge rotation). */
  readonly extra?: string;
  /** Class added to every emitted face, for CSS hooks. */
  readonly className?: string;
}

const px = (value: number): string => `${value.toFixed(2)}px`;

/** One flat surface: an absolutely-positioned element mapped into 3D. */
export const face = (width: number, height: number, transform: string, background: string, className = ""): HTMLElement => {
  const el = document.createElement("i");
  el.className = `f ${className}`.trim();
  el.style.width = px(width);
  el.style.height = px(height);
  el.style.transform = transform;
  el.style.background = background;
  return el;
};

/**
 * A rectangular solid as up to four CSS 3D planes. Returns a `preserve-3d`
 * wrapper so the caller can re-pose the whole solid with a single style write —
 * the difference between one transform per object and one per face.
 */
export const solid = (box: SolidBox, paint: SolidPaint, options: SolidOptions = {}): HTMLElement => {
  const { x, y, z, w, d, h } = box;
  const top = z + h;
  const wrap = document.createElement("div");
  wrap.className = `s ${options.className ?? ""}`.trim();
  const after = options.extra === undefined ? "" : ` ${options.extra}`;

  const wants = (flag: boolean | undefined): boolean => flag !== false;
  if (wants(options.top) && h >= 0) {
    wrap.append(face(w, d, `translate3d(${px(x)},${px(y)},${px(top)})${after}`, paint.top));
  }
  if (wants(options.near)) {
    wrap.append(face(w, h, `translate3d(${px(x)},${px(y + d)},${px(top)}) rotateX(-90deg)${after}`, paint.near));
  }
  if (wants(options.left)) {
    wrap.append(
      face(d, h, `translate3d(${px(x)},${px(y)},${px(top)}) rotateX(-90deg) rotateY(-90deg)${after}`, paint.side),
    );
  }
  if (wants(options.right)) {
    wrap.append(
      face(d, h, `translate3d(${px(x + w)},${px(y + d)},${px(top)}) rotateX(-90deg) rotateY(90deg)${after}`, paint.side),
    );
  }
  return wrap;
};

/** A flat element lying on the ground plane (z = height), unrotated — used for
 * the lagoon, contact shadows and sand patches. Ground quads are the cheapest
 * possible surface: no rotation, no extra compositing layer. */
export const groundQuad = (
  cx: number,
  cy: number,
  width: number,
  depth: number,
  height: number,
  background: string,
  className = "",
): HTMLElement => {
  const el = face(width, depth, `translate3d(${px(cx - width / 2)},${px(cy - depth / 2)},${px(height)})`, background, className);
  return el;
};

/** A `preserve-3d` group the caller can transform as one unit. */
export const group = (className = ""): HTMLElement => {
  const el = document.createElement("div");
  el.className = `s ${className}`.trim();
  return el;
};

/** Place a group at a world position, with an optional extra transform. */
export const placeGroup = (el: HTMLElement, x: number, y: number, z: number, extra = ""): void => {
  el.style.transform = `translate3d(${px(x)},${px(y)},${px(z)})${extra === "" ? "" : ` ${extra}`}`;
};
