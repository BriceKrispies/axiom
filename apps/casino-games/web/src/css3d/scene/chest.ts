/*
 * chest.ts — LAYER 2a of the CSS3D build: one treasure chest as a CSS 3D solid.
 *
 * The engine scene spends 41 nodes (~246 CSS faces) on a chest, because it is
 * authored for a GPU that does not care. A DOM renderer pays per element, so
 * this chest is authored the other way round: 13 elements that reproduce the
 * reference's read — carved wooden body, banded domed lid, gold straps, latch,
 * contact shadow — and nothing that would not survive at that budget.
 *
 * The chest is a NESTED transform tree, which is what makes it cheap to animate:
 *
 *     .chest        grid slot + idle bob + lift        <- one style write
 *       .shadow     ground quad
 *       .body       3 faces + 2 straps + latch
 *       .lidPivot   rotateX() about the FAR-TOP hinge  <- one style write
 *         .lid      4 faces + crown + band + plate
 *       .prize      the reward that rises on a win
 *
 * Opening the lid is a single transform on `.lidPivot`; the eight elements below
 * it are never touched. The lid geometry is authored with its hinge at the local
 * origin precisely so that rotation is a one-property animation.
 */

import { face, group, placeGroup, solid } from "../render/solid.ts";

/**
 * The reference's carved-oak value ladder: lid catches the key light, front
 * boards sit mid, side boards fall to the shadow value. With no textures, that
 * value step IS what reads as stacked planks.
 *
 * Where the engine build spends GEOMETRY on detail — eight lid-arc slats, three
 * groove boxes per chest — this build spends GRADIENTS, which cost nothing. The
 * body's plank seams are a `repeating-linear-gradient`, and the lid's barrel
 * curve is a vertical ramp on one flat quad. Same read, zero extra elements.
 */
const PLANKS = "repeating-linear-gradient(90deg,#8d5a2a 0 25px,#89562764 25px 27px,#7a4c22 27px 28px)";
const LID_CURVE = "linear-gradient(180deg,#bb8244 0%,#a97438 34%,#96632f 70%,#7d4f24 100%)";
const WOOD_LID = { near: LID_CURVE, side: "#7d4f24", top: "linear-gradient(180deg,#cb9451 0%,#b07a3d 100%)" };
const WOOD_BODY = { near: PLANKS, side: "#6d431d", top: "#a06a35" };
const GOLD = { near: "linear-gradient(180deg,#f6d36a 0%,#e8b53f 60%,#c9942c 100%)", side: "#c9942c", top: "#f6d36a" };
const GOLD_BRIGHT = { near: "radial-gradient(ellipse at 50% 34%,#fff0a8 0%,#ffd964 52%,#dca832 100%)", side: "#e0ae3c", top: "#fff0a8" };

/** World dimensions, in the same px-per-unit space the diorama lays out in. */
export const CHEST = { bodyH: 34, d: 68, lidH: 27, w: 104 };

/** How far a strap/plate stands proud of the face it decorates, so it never
 * z-fights the board behind it. */
const PROUD = 0.6;

export interface ChestView {
  readonly el: HTMLElement;
  /** Idle bob + hover lift + the winning chest's rise, as one transform. */
  readonly pose: (bob: number, lift: number, twist: number) => void;
  /** 0 = shut, 1 = fully open. */
  readonly open: (amount: number) => void;
  readonly setFocused: (focused: boolean) => void;
  /** Reveal the prize (or the empty interior) once the lid is up. */
  readonly setPrize: (label: string | null, won: boolean) => void;
  readonly setDimmed: (dimmed: boolean) => void;
}

/** Build one chest. `brand` stamps the centre chest's nameplate. */
export const buildChest = (slotX: number, slotY: number, brand: string | null): ChestView => {
  const { w, d, bodyH, lidH } = CHEST;
  const root = group("chest");

  // ── contact shadow: a soft ground quad, the cheapest possible grounding cue
  const shadow = face(
    w * 1.16,
    d * 1.05,
    `translate3d(${(-w * 0.58).toFixed(2)}px,${(-d * 0.52).toFixed(2)}px,0.4px)`,
    "radial-gradient(ellipse at 50% 50%, rgba(20,44,52,.42) 0%, rgba(20,44,52,.24) 46%, rgba(20,44,52,0) 72%)",
    "shadow",
  );
  root.append(shadow);

  // ── body: three visible boards. The far face and underside can never be seen.
  const body = solid(
    { d, h: bodyH, w, x: -w / 2, y: -d / 2, z: 0 },
    WOOD_BODY,
    { className: "body", top: false },
  );
  root.append(body);

  // Two gold straps down the near face + the latch, each standing PROUD of it.
  const strapAt = (offset: number): HTMLElement =>
    face(
      10,
      bodyH,
      `translate3d(${(offset - 5).toFixed(2)}px,${(d / 2 + PROUD).toFixed(2)}px,${bodyH}px) rotateX(-90deg)`,
      GOLD.near,
      "strap",
    );
  body.append(strapAt(-w * 0.3), strapAt(w * 0.3));
  // The latch: a small bright plate straddling the lid seam, the one spot of
  // specular gold the reference puts at the centre of every chest.
  body.append(
    face(
      20,
      13,
      `translate3d(-10px,${(d / 2 + PROUD * 2).toFixed(2)}px,${(bodyH - 2).toFixed(2)}px) rotateX(-90deg)`,
      GOLD_BRIGHT.near,
      "latch",
    ),
  );

  // ── lid: hinged at the FAR-TOP edge, so `rotateX` on the pivot opens it.
  const lidPivot = group("lidPivot");
  lidPivot.style.transform = "rotateX(0deg)";
  placeGroup(lidPivot, 0, -d / 2, bodyH);

  // Lid-local space: y runs 0..d away from the hinge, z runs 0..lidH.
  const lid = solid({ d, h: lidH * 0.62, w, x: -w / 2, y: 0, z: 0 }, WOOD_LID, { className: "lid" });
  // A narrower crown slab stacked on top reads as the domed lid of the reference
  // without paying for the engine's eight-slat arc.
  const crown = solid(
    { d: d * 0.74, h: lidH * 0.38, w: w * 0.9, x: (-w * 0.9) / 2, y: d * 0.13, z: lidH * 0.62 },
    WOOD_LID,
    { className: "crown" },
  );
  // Gold trim along the lid's lower edge, plus two lid straps that line up with
  // the body straps so each band reads as ONE strap wrapping the whole chest.
  const lidFrontH = lidH * 0.62;
  const trim = face(
    w,
    6,
    `translate3d(${(-w / 2).toFixed(2)}px,${(d + PROUD).toFixed(2)}px,6px) rotateX(-90deg)`,
    "linear-gradient(180deg,#f6d36a 0%,#d9a636 100%)",
    "trim",
  );
  const lidStrap = (offset: number): HTMLElement =>
    face(
      10,
      lidFrontH,
      `translate3d(${(offset - 5).toFixed(2)}px,${(d + PROUD).toFixed(2)}px,${lidFrontH.toFixed(2)}px) rotateX(-90deg)`,
      GOLD.near,
      "strap",
    );
  lidPivot.append(lid, crown, trim, lidStrap(-w * 0.3), lidStrap(w * 0.3));

  // The centre chest carries the brand nameplate — one element with real text,
  // where the engine scene welds 23 stroke boxes to spell the same word.
  if (brand !== null) {
    const plate = document.createElement("i");
    plate.className = "f plate";
    plate.textContent = brand;
    plate.style.width = `${(w * 0.62).toFixed(2)}px`;
    plate.style.height = "20px";
    plate.style.transform = `translate3d(${(-w * 0.31).toFixed(2)}px,${(d + PROUD * 3).toFixed(2)}px,${(lidH * 0.52).toFixed(2)}px) rotateX(-90deg)`;
    lidPivot.append(plate);
  }
  root.append(lidPivot);

  // ── the prize that rises out of an opened chest
  const prize = document.createElement("i");
  prize.className = "f prize";
  prize.style.width = "44px";
  prize.style.height = "44px";
  prize.style.transform = `translate3d(-22px,0,${bodyH + 6}px) rotateX(-58deg)`;
  root.append(prize);

  let openAmount = 0;
  return {
    el: root,
    open: (amount: number): void => {
      openAmount = amount;
      lidPivot.style.transform = `translate3d(0,${(-d / 2).toFixed(2)}px,${bodyH}px) rotateX(${(amount * 104).toFixed(1)}deg)`;
    },
    pose: (bob: number, lift: number, twist: number): void => {
      root.style.transform =
        `translate3d(${slotX.toFixed(2)}px,${slotY.toFixed(2)}px,${(bob + lift).toFixed(2)}px)` +
        ` rotateZ(${twist.toFixed(2)}deg)`;
    },
    setDimmed: (dimmed: boolean): void => {
      root.classList.toggle("is-dim", dimmed);
    },
    setFocused: (focused: boolean): void => {
      root.classList.toggle("is-focus", focused);
    },
    setPrize: (label: string | null, won: boolean): void => {
      prize.textContent = label ?? "";
      prize.classList.toggle("is-shown", label !== null && openAmount > 0.35);
      prize.classList.toggle("is-win", won);
    },
  };
};
