/*
 * diorama.ts — LAYER 2b of the CSS3D build: the beach the chests sit in.
 *
 * Sand slab, lagoon, palm, sandcastle, crab and shore litter — every one a CSS
 * 3D solid or a ground quad, ~40 elements for the whole set. All of it is
 * STATIC: nothing here is re-posed per frame, so once the browser has composited
 * it the cost is zero. That is the single biggest reason this build holds 60fps
 * where the generic backend does not.
 *
 * THE WATER. In the engine app the lagoon's cell pattern is painted by a
 * Canvas2D overlay (`drawStylizedWaterSurface`). There is no canvas here, so the
 * lagoon is rebuilt out of CSS: an elliptical ground quad carrying a radial
 * depth gradient, a hex cell net woven from three `repeating-linear-gradient`s
 * at 60° to each other, a soft shoreline fade, and two slow CSS-animated glints.
 * No JS runs per frame for any of it — the animation is on the compositor.
 */

import { face, group, solid } from "../render/solid.ts";

const SAND = { near: "#cf9a5f", side: "#b8834b", top: "#dcaa6c" };
const CASTLE = { near: "#e8dcb6", side: "#cfc096", top: "#f3ebcd" };
const PALM_BARK = { near: "#7a5230", side: "#5f3e23", top: "#8d6039" };

/** The lagoon's radii, in world units. */
export const LAGOON = { rx: 362, ry: 254 };

/** Three 60°-separated line grids overlaid = a hexagonal cell net, the CSS
 * stand-in for the canvas water pattern. Kept very low-contrast: it should read
 * as surface texture, never as a graphic. */
const HEX_NET = [0, 60, 120]
  .map(
    (angle) =>
      `repeating-linear-gradient(${angle}deg, rgba(255,255,255,.055) 0 1.5px, rgba(255,255,255,0) 1.5px 34px)`,
  )
  .join(",");

/** The full static set-dressing, as one `preserve-3d` group. */
export const buildDiorama = (): HTMLElement => {
  const root = group("diorama");

  // ── the sand slab: one big ground quad with a warm vignette toward the edges
  root.append(
    face(
      3200,
      2600,
      "translate3d(-1600px,-1500px,0px)",
      "radial-gradient(ellipse 30% 26% at 50% 62%, #e6b273 0%, #dca768 44%, #c9924f 72%, #b07c3e 100%)",
      "sand",
    ),
  );

  // ── the lagoon: base water, hex net, shoreline fade, glints
  const water = face(
    LAGOON.rx * 2,
    LAGOON.ry * 2,
    `translate3d(${-LAGOON.rx}px,${-LAGOON.ry}px,1px)`,
    "radial-gradient(ellipse at 50% 46%, #3fbdb0 0%, #2ea79c 44%, #1f8d87 78%, #17787a 100%)",
    "water",
  );
  root.append(water);
  const netEl = face(
    LAGOON.rx * 2,
    LAGOON.ry * 2,
    `translate3d(${-LAGOON.rx}px,${-LAGOON.ry}px,1.4px)`,
    HEX_NET,
    "water-net",
  );
  root.append(netEl);
  root.append(
    face(
      LAGOON.rx * 2.12,
      LAGOON.ry * 2.14,
      `translate3d(${(-LAGOON.rx * 1.06).toFixed(1)}px,${(-LAGOON.ry * 1.07).toFixed(1)}px,0.8px)`,
      "radial-gradient(ellipse at 50% 50%, rgba(31,141,135,0) 62%, rgba(46,167,156,.55) 78%, rgba(220,170,108,0) 92%)",
      "shore",
    ),
  );
  root.append(
    face(
      LAGOON.rx * 1.1,
      LAGOON.ry * 0.62,
      `translate3d(${(-LAGOON.rx * 0.72).toFixed(1)}px,${(-LAGOON.ry * 0.66).toFixed(1)}px,1.8px)`,
      "radial-gradient(ellipse at 50% 50%, rgba(255,255,255,.20) 0%, rgba(255,255,255,0) 70%)",
      "glint glint-a",
    ),
  );
  root.append(
    face(
      LAGOON.rx * 0.86,
      LAGOON.ry * 0.5,
      `translate3d(${(-LAGOON.rx * 0.1).toFixed(1)}px,${(LAGOON.ry * 0.22).toFixed(1)}px,1.8px)`,
      "radial-gradient(ellipse at 50% 50%, rgba(255,255,255,.14) 0%, rgba(255,255,255,0) 70%)",
      "glint glint-b",
    ),
  );

  // ── palm at the far left: leaning trunk + a fan of fronds + coconuts
  const palm = group("palm");
  palm.style.transform = "translate3d(-408px,-168px,0px)";
  palm.append(
    face(
      130,
      86,
      "translate3d(-65px,-43px,0.6px)",
      "radial-gradient(ellipse at 50% 50%, rgba(20,44,52,.34) 0%, rgba(20,44,52,0) 70%)",
    ),
  );
  // The trunk tapers by stacking two shorter solids — a segmented palm bole for
  // six faces total, where the engine build spends four cylinders (192 faces).
  palm.append(solid({ d: 32, h: 74, w: 36, x: -18, y: -16, z: 0 }, PALM_BARK, { top: false }));
  palm.append(solid({ d: 26, h: 66, w: 29, x: -14.5, y: -13, z: 72 }, PALM_BARK, { top: false }));

  // Fronds fan RADIALLY in the ground plane about the crown. `transform-origin`
  // is the element's own corner, which after the translate sits at the crown
  // centre — so `rotateZ` sweeps each blade out like the spokes of a parasol,
  // and the world tilt foreshortens them into a convincing crown.
  const CROWN_Z = 132;
  [-70, -34, 2, 38, 74, 118, 160, -140].forEach((angle, i) => {
    const long = i % 2 === 0 ? 96 : 78;
    palm.append(
      face(
        long,
        30,
        `translate3d(0px,-15px,${CROWN_Z}px) rotateZ(${angle}deg) rotateY(${i % 2 === 0 ? 14 : 20}deg)`,
        i % 2 === 0
          ? "linear-gradient(90deg,#4b9a3c 0%,#5cae49 62%,#6dc158 100%)"
          : "linear-gradient(90deg,#3d8531 0%,#4f9b3e 62%,#5cae49 100%)",
        "frond",
      ),
    );
  });
  [
    [-13, -5],
    [10, -9],
    [-1, 8],
  ].forEach(([cx, cy]) => {
    palm.append(
      face(
        16,
        16,
        `translate3d(${cx}px,${cy}px,${CROWN_Z - 6}px)`,
        "radial-gradient(circle at 34% 30%, #7a5533 0%, #4a3120 72%)",
        "coconut",
      ),
    );
  });
  root.append(palm);

  // ── sandcastle at the far right: base, three towers, a flag
  const castle = group("castle");
  castle.style.transform = "translate3d(392px,-176px,0px)";
  castle.append(
    face(
      190,
      126,
      "translate3d(-95px,-63px,0.6px)",
      "radial-gradient(ellipse at 50% 50%, rgba(20,44,52,.3) 0%, rgba(20,44,52,0) 72%)",
    ),
  );
  castle.append(solid({ d: 92, h: 18, w: 158, x: -79, y: -46, z: 0 }, CASTLE, { top: true }));
  // Three turrets. Their battlements are a `clip-path` notch applied in CSS to
  // the turret's own faces, rather than the engine build's eighteen crenel boxes.
  [
    [-46, 6, 68],
    [4, -12, 92],
    [48, 10, 60],
  ].forEach(([cx, cy, th]) => {
    castle.append(
      solid(
        { d: 32, h: th as number, w: 32, x: (cx as number) - 16, y: (cy as number) - 16, z: 18 },
        CASTLE,
        { className: "tower" },
      ),
    );
  });
  castle.append(
    face(5, 48, "translate3d(1.5px,-12px,110px) rotateX(-90deg)", "#8d6039", "pole"),
    face(32, 21, "translate3d(6px,-12px,156px) rotateX(-90deg)", "#e0452e", "flag"),
  );
  root.append(castle);

  // ── crab on the near-left shore
  const crab = group("crab");
  crab.style.transform = "translate3d(-300px,214px,0px)";
  crab.append(
    face(78, 54, "translate3d(-39px,-27px,0.6px)", "radial-gradient(ellipse at 50% 50%, rgba(20,44,52,.3) 0%, rgba(20,44,52,0) 72%)"),
  );
  crab.append(face(52, 38, "translate3d(-26px,-19px,14px)", "radial-gradient(circle at 38% 32%, #e8583c 0%, #cf3f28 62%, #a92f1c 100%)", "crab-body"));
  [-1, 1].forEach((s) => {
    crab.append(
      face(20, 16, `translate3d(${s * 28 - 10}px,-4px,12px) rotateZ(${s * 22}deg)`, "radial-gradient(circle at 40% 34%, #e8583c 0%, #b8341f 100%)", "claw"),
    );
    [0, 1, 2].forEach((li) => {
      crab.append(face(15, 4, `translate3d(${s * 22 - 7}px,${6 + li * 8}px,10px) rotateZ(${s * (14 + li * 10)}deg)`, "#c33a24", "leg"));
    });
    crab.append(face(7, 7, `translate3d(${s * 9 - 3}px,-16px,20px)`, "#2a1410", "eye"));
  });
  root.append(crab);

  // ── shore litter: a few shells and a starfish, pure decoration
  const LITTER: readonly (readonly [number, number, number, string])[] = [
    [-470, 66, 15, "#f3ead6"],
    [468, 120, 13, "#f7efdd"],
    [-250, -300, 12, "#efe2c8"],
    [300, -318, 14, "#f3ead6"],
    [104, 330, 16, "#f0c98f"],
  ];
  LITTER.forEach(([lx, ly, size, color]) => {
    root.append(
      face(size, size * 0.8, `translate3d(${lx}px,${ly}px,1.2px)`, `radial-gradient(circle at 40% 34%, ${color} 0%, #cbb894 100%)`, "shell"),
    );
  });

  return root;
};
