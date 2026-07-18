// Axiom demo gallery — repo tooling (NOT part of the engine dependency graph;
// same status as the Makefile and scripts/). This module owns the demo manifest
// for the static showcase landing grid. It is a plain ES module served
// statically; it imports nothing from the engine.
//
// Every demo is a standalone app under apps/ that owns its page and its own
// wasm bundle (or, for the pure-TS games, a committed single-file page).
// scripts/package_gallery.py (`make gallery`) copies this landing grid into
// dist/, copies each Rust app's web/ into dist/<id>/, and builds that app's
// capability-detecting loader (axiom-loader.js + wasm) beside its page. A card
// is therefore always just a link to `<id>/index.html` — there is no shared
// shell and no shared wasm bundle anymore. To work on one demo with hot
// reload, use `cargo run -p axiom-serve -- <id>` instead of packaging.

// The gallery manifest: one card per demo, each linking to the page the demo
// app owns. `page` is relative to the dist root.
export const DEMOS = [
  {
    id: "rotating-cube",
    title: "Rotating Cube",
    blurb: "Three deterministic shaded cubes spinning on different axes.",
    desc:
      "The engine's browser-visible vertical slice: pure scene description on " +
      "App::new()…run(), rendered through WebGPU. Purely visual — no input.",
    page: "rotating-cube/index.html",
  },
  {
    id: "home-run",
    title: "Home Run!",
    blurb: "An arcade batting contest on a toy diamond: load the bat, read the pitch, clear the blue wall.",
    desc:
      "A toy-tabletop arcade baseball batting game authored purely in TypeScript on the engine's " +
      "3D scene surface — a fixed elevated camera behind home plate frames a compact striped diamond " +
      "with brown base paths, white foul lines, a pitching machine on the mound, blue stadium walls, " +
      "and nine red toy fielders wandering their own patrol circles. Ten pitches per round from a " +
      "deterministic seeded sequence — slow balls, sinkers, heaters, risers, inside and outside looks, " +
      "each telegraphed by the machine's compression. A/D shift the batter inside the box; the batter " +
      "idles wound at FULL POWER — one SPACE press fires the max-power swing instantly, then he " +
      "re-winds on his own (the swing cooldown, shown by a fading ready meter). Contact is resolved " +
      "from the real spatial sweep of bat vs ball — position " +
      "along the barrel, timing angle, and vertical offset decide exit speed, spray, and loft — so " +
      "mistimed swings foul off, jam, top grounders, or pop up, while a square, well-positioned strike " +
      "clears the wall for HOME RUN! (500 + distance, consecutive homers multiply). Fielders converge " +
      "on reachable landing points and rob weak hits. Deterministic under fixed-step replay.",
    // Pure-TS game: committed single-file page — regenerate with `make gallery-home-run`.
    page: "home-run/index.html",
  },
  {
    id: "three-point",
    title: "Three-Point Shootout",
    blurb: "A first-person three-point rack contest: ride the rise and release at the top — or swipe the ball up on touch.",
    desc:
      "A first-person 3D three-point contest in the spirit of Wii Sports rack shooting — a FULLY " +
      "SELF-CONTAINED pure-TypeScript app that ships its own engine (WebGL2 forward renderer, " +
      "fixed-step loop, pointer-lock/touch input, WebAudio synth) with no SDK and no wasm; the " +
      "whole game is one 70 KB page. Fifteen shots from three spots " +
      "around a procedurally-built arc — left wing, top of the key, right wing — five balls per " +
      "rack with a golden fifth ball. Every shot is ONE continuous motion that never waits: the " +
      "moment you release, the next ball is dealt off its rack slot into your hands while the " +
      "last shots are still in the air (several fly at once, scored in shot order). Holding " +
      "SPACE rises into the shot and releasing launches at that exact instant — the shot meter " +
      "tracks the rise and its ideal window. Early is short, the ideal window swishes, late " +
      "clangs off the glass. On touch, drag to look and swipe up from the " +
      "held ball to shoot — flick strength is your release, sideways flick steers, with the " +
      "same smoothed-gesture model as Swipe Basketball. The camera is exclusively player-driven " +
      "(the game never touches your view), so skill is your aim plus release timing. The ball " +
      "is a genuinely simulated projectile (deterministic fixed-step integrator — gravity, " +
      "backspin, restitution) against a rim whose colliders match the visible torus exactly; " +
      "baskets are confirmed by a two-plane downward-crossing detector; streaks compound " +
      "(3, 6, 9, 12…) and a miss resets them. After ball 15 the buzzer shows your line; R runs " +
      "it back. Deterministic under fixed-step replay, with a headless agent driver that plays " +
      "full games in Node.",
    // Pure-TS game: committed single-file page — regenerate with `make gallery-three-point`.
    page: "three-point/index.html",
  },
  {
    id: "end-zone",
    title: "End Zone",
    blurb: "An original arcade-football game: pick a team, snap the ball, fire the pass, and take the end zone.",
    desc:
      "A complete arcade-football experience on the engine — a full early-2000s arcade " +
      "front-end (chrome END ZONE mark, attract mode, team select across six original " +
      "fictional teams with procedural emblems and real POWER/SPEED/PASS/DEFENSE ratings, " +
      "a VS matchup screen, five categories of working settings with persistence, pause " +
      "and rebindable controls) over a deterministic 7-on-7 football simulation: formation, " +
      "snap, drop-back, routes, blocking, a real physics pass, catch, pursuit, tackle, and " +
      "ground impact, directed by an event-driven camera with impact juice. Menus: arrows/WASD " +
      "move, Enter selects, Esc backs out. In the match: WASD/arrows steer your ball " +
      "handler, Enter snaps then throws (the quarterback never throws on his own), P pauses. " +
      "On touch: a virtual joystick plus SNAP·THROW and PAUSE buttons. Every visual is " +
      "procedural — no image assets. Deterministic under fixed-step replay.",
    page: "end-zone/index.html",
  },
  {
    id: "growth",
    title: "Growth (walkable terrain)",
    blurb: "Generate a planet, pick a spot on its map, and walk the procedural terrain in first person.",
    desc:
      "A procedural-terrain world viewer on the engine: configure and generate a " +
      "planet, descend onto a land spot from the overworld map, then walk its " +
      "streamed LOD terrain. Desktop: click the canvas to capture the mouse, WASD/" +
      "arrows to move, mouse to look, Esc to release. (WebGPU; desktop-oriented.)",
    page: "growth/index.html",
  },
  {
    id: "generia",
    title: "Generia (forest)",
    blurb: "Walk an Axiom-rendered procedural forest in first person — the fall-forest game, ported onto the engine.",
    desc:
      "A first-person walk through a procedural forest rendered with the engine's " +
      "GPU forest pipeline (terrain, trees, foliage, ground clutter, fog). Click the " +
      "canvas to capture the mouse, WASD/arrows to move, mouse to look, Esc to release. " +
      "The port foundation for the fall-forest game (streaming, props, discoveries, " +
      "and world modes land in later phases).",
    page: "generia/index.html",
  },
  {
    id: "forest-walk",
    title: "Forest Walk",
    blurb: "Walk the visual-target forest diorama in first person — generia's predecessor, kept walkable.",
    desc:
      "A first-person walk through the Visual Target 001 forest diorama (the " +
      "postcard scene the visual-convergence loop scores against), built on the " +
      "engine's terrain mesher and first-person controller. Click the canvas to " +
      "capture the mouse, WASD/arrows to move, mouse to look, Esc to release.",
    page: "forest-walk/index.html",
  },
  {
    id: "zanzoban",
    title: "Zanzoban",
    blurb: "Leave ghosts of your past runs on the buttons, then walk the live block through the doors they open.",
    desc:
      "A deterministic top-down grid puzzle on the engine. Walk a block one cell " +
      "at a time (WASD / arrows); press Q to freeze the current run into a ghost " +
      "that replays your exact path on a fixed 0.5s step, and R to restart. " +
      "Ghosts are solid and hold buttons open — so the way through a locked door " +
      "is to leave a ghost on the button and walk the live block through. " +
      "Includes an in-browser level editor + playtest with TOML import/export.",
    page: "zanzoban/index.html",
  },
  {
    id: "quintet",
    title: "Quintet",
    blurb: "Press the board to place 5-cell blocks on a 10×10 grid and fill rows and columns to clear them for score.",
    desc:
      "A deterministic block-breaking placement game on the engine. Press the " +
      "board to summon the generated quintet (a 5-cell polyomino) under your " +
      "cursor — or drag it from the side panel — and release to place it on the 10×10 " +
      "board; fill any whole row or column to clear it, and clear several lines at " +
      "once for bonus points. Every offered piece is a real orthogonally-connected " +
      "pentomino guaranteed to fit somewhere — generation is seeded from the board, " +
      "score, and move count, so a given state always yields the same next piece. " +
      "When nothing fits, the board reports a stuck state and you press Reset.",
    page: "quintet/index.html",
  },
  {
    id: "gravix",
    title: "Gravix",
    blurb: "Roll a physics marble across procedurally-generated floating platform courses — over ramps, across jump gaps — collecting coins to the finish pad.",
    desc:
      "A marble-roll platformer on the engine's deterministic rigid-body physics. " +
      "Steer with camera-relative roll torque (W A S D): the contact-point friction " +
      "converts spin into real forward rolling, so the marble carries momentum. " +
      "Space jumps when grounded, Shift brakes, and the arrow keys orbit the camera. " +
      "Every course is procedurally generated from its level index — a winding grid " +
      "path with turns, tilted ramps (oriented-box collision), jump gaps, and hovering " +
      "coins — so each level replays identically. Reach the finish pad to advance; three " +
      "falls end the run (press R to restart). (WebGPU, with a WebGL2 / Canvas2D fallback.)",
    page: "gravix/index.html",
  },
  {
    id: "sports-physics-lab",
    title: "Sports Physics Lab",
    blurb: "A first-person procedural sports arena: walk the field, pick up four kinds of sports balls (and the practice dummy), and toss them with real physics.",
    desc:
      "The foundational interactive sports primitive lab. A procedurally generated " +
      "60×90 practice field (markings baked in code) enclosed by bouncy walls; a " +
      "lineup of four procedural balls — soccer, football, bowling, baseball — each " +
      "a real rigid body with its own mass, bounce, and friction; and a T-pose " +
      "humanoid practice dummy. Click the canvas to capture the mouse and look " +
      "around; W A S D walk; left click picks up what the reticle targets and " +
      "tosses what you hold (heavier objects throw slower); right click sets it " +
      "down gently; V or the mouse wheel zooms out to third person to see your own " +
      "procedural body; R resets the lineup. Everything visible is generated at " +
      "runtime — no imported assets. (WebGPU, with a WebGL2 / Canvas2D fallback.)",
    page: "sports-physics-lab/index.html",
  },
];

/** Look a demo up by its `id`, or `null` when unknown. */
export function demoById(id) {
  return DEMOS.find((d) => d.id === id) || null;
}
