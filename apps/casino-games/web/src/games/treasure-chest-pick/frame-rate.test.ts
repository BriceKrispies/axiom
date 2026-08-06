/*
 * frame-rate.test.ts — the chest game's END-TO-END frame-rate regression test,
 * on the Canvas2D software rasterizer, headless.
 *
 * It plays a whole round the way a player does: boot the game, wait out the
 * intro, CLICK a chest, then watch the crab fetch it, the chest spiral into its
 * close-up, the latch drop, the lid open and the treasure rise — and it times
 * every single frame of that while it happens.
 *
 * "End to end" is the point. `web/browser/bench_render.py` already times the
 * renderer alone on a frozen scene, which is the right tool for judging a
 * rasterizer change; it deliberately removes the game. This is the other number:
 * what the player's machine has to do per frame, with the fold, the scene author,
 * the reconciler, the overlay geometry and the rasterizer all in it. That is the
 * number `definition.ts` quotes when it explains why `renderScale` defaults to
 * 0.5, and until now nothing held it.
 *
 * Everything below the harness is the SHIPPED code path — `TREASURE_CHEST_PICK.mount`
 * → `mountCasinoGame` → `runGame` → `initRenderer(…, "canvas2d", …)` → the real
 * z-buffered scanline rasterizer writing a real framebuffer. See
 * `../headless-canvas2d.testkit.ts` for exactly what is real and what is stubbed
 * (the framebuffer present and the overlay's vector fills are counted, not
 * rasterized), and why the engine's clock is virtual while the measurement is not.
 *
 * The treasure is PINNED rather than drawn: `InjectedChanceResultSource` is the
 * shipped source for an authoritative service, and supplying it one outcome makes
 * the round — and therefore the work the frames do — identical on every run. One
 * treasure is enough; the reveal ritual is the same ceremony for all five, and
 * `gold-coin` is the common case a player mostly sees. Its `common` rarity also
 * makes this the LEAST expensive celebration, so the floors below are the ones
 * that must hold at minimum.
 */

import assert from "node:assert/strict";
import test from "node:test";

import { rendererBackendName, rendererNodeCount } from "@axiom/web-engine";
import type { CasinoHud } from "../../chance-engine/registry/definition.ts";
import { InjectedChanceResultSource } from "../../chance-engine/outcomes/result-source.ts";
import { CANVAS_HEIGHT, CANVAS_WIDTH, worldToCanvas } from "../../presentation/cameras/picking.ts";
import { installHeadlessCanvas2dBrowser } from "../headless-canvas2d.testkit.ts";
import { CHEST_TIMING, chestCamera, chestPosition, commitBeatTicks, revealTimeline } from "./game.ts";
import { TREASURE_CHEST_PICK } from "./definition.ts";

/** The CSS box `definition.ts` quotes its measured frame rates at. */
const CSS_WIDTH = 936;
const CSS_HEIGHT = 585;

/** The centre chest of the 3x3 board — the one furthest from every draggable
 * beach prop, so the click cannot be swallowed by a prop grab. */
const PICKED_CHEST = 4;
const CHEST_COUNT = 9;

/** The treasure this round is committed to hold. */
const PINNED_TIER = "gold-coin";
const PRESENTATION_SEED = 0x5eed_beef;

/** Ticks the scripted click is spread over. The intro lasts `INTRO_TICKS` (24),
 * so tick 30 is comfortably inside "ready"; the three beats are what a real
 * click is: the cursor rests on the chest (which ARMS it — chest picking is
 * tap-to-confirm, and a desktop hover is the first of the two taps), the button
 * goes down, the button comes up. The release on an armed, still-hovered target
 * is the selection. */
const HOVER_TICK = 30;
const PRESS_TICK = 34;
const RELEASE_TICK = 35;

/** A generous ceiling on frames pumped, so a game that never completes fails as
 * a test rather than hanging. The round's real length is asserted against the
 * timeline constants below. */
const MAX_FRAMES = 900;

/**
 * The frame-rate floor, in frames per second, for the whole round end to end.
 *
 * This is a RATCHET, exactly like the Rust render-churn gate in CI: raise it as
 * the software path gets faster, never lower it to make a change pass. It is set
 * well under what a development machine measures (which is ~55-60 fps at this
 * quality) because it has to survive a shared CI runner without flaking — what it
 * is built to catch is the kind of regression that costs a multiple, not a few
 * percent: a per-frame allocation in the rasterizer's inner loop, a whole-node
 * cull that stops culling, a scene author that re-spawns instead of re-posing.
 *
 * Override for a local measurement on a busy machine: AXIOM_CHEST_FPS_FLOOR=10.
 */
const FPS_FLOOR = Number(process.env["AXIOM_CHEST_FPS_FLOOR"] ?? 15);

/** Frames excluded from the steady-state hitch check: the backend allocates its
 * framebuffer and the store creates every mesh and material on the first frames,
 * and paying that once is correct, not a hitch. */
const WARMUP_FRAMES = 24;

/** How far over the median a single steady-state frame may go before it counts as
 * a visible hitch rather than scheduler noise. Also a ratchet — tighten it. */
const HITCH_LIMIT = 12;

/**
 * The SCENE COST CEILING: the most retained scene nodes any frame of the round
 * may present, and the most node-frames the whole round may cost.
 *
 * This is the guard that fps cannot be. Wall-clock frame time on a developer
 * machine drifts by well over 50% between runs — `web/browser/bench_rasterizer.py`
 * documents the same unchanged code measuring 20.6ms and 34.1ms minutes apart —
 * so a before/after fps comparison cannot see a regression worth less than about
 * 2x. Node count has no such noise: it is an exact, deterministic integer, and
 * the software rasterizer costs very nearly one unit of time per node per frame
 * plus one per backing pixel.
 *
 * So this is where a visual change that quietly makes the SOFTWARE path more
 * expensive gets caught — a scene author that adds props, a reveal that spawns
 * more burst motes, a decorative layer added for the hardware path and left
 * switched on for both. It is a CEILING, not an equality: a change is free to
 * spend fewer nodes. Raising it is a deliberate decision about the software
 * path's budget, made with the fps report in hand — never a reflex to make a
 * proposal land.
 *
 * The headroom is deliberately THIN (~3% over what the scene costs today) rather
 * than generous, because there is nothing to absorb: this is an integer that does
 * not move between runs. A camera, grade, exposure, fog or light-rig change moves
 * it by exactly zero and passes freely; only added GEOMETRY trips it. That
 * asymmetry is the point — the visual axes that cost the software path nothing
 * should be unconstrained here, and the one that costs it dearly should not be.
 *
 * Note what it does NOT cover: the same nodes carrying finer meshes. Tessellation
 * is per-node and invisible here, so a `segments` increase shows up only in the
 * frame time. If nodes are flat and fps falls, that is where to look.
 */
const MAX_NODES_PER_FRAME = 410;
const MAX_NODE_FRAMES = 128_000;

/** Per-phase frame times and scene sizes, so the report says WHERE the time goes
 * and WHAT the frame was carrying when it went there. */
interface Segment {
  readonly label: string;
  readonly frameMs: number[];
  readonly nodes: number[];
}

const summarize = (frameMs: readonly number[]): { readonly mean: number; readonly median: number; readonly p95: number; readonly max: number } => {
  const sorted = [...frameMs].sort((a, b) => a - b);
  const at = (fraction: number): number => sorted[Math.min(sorted.length - 1, Math.floor(fraction * sorted.length))] ?? 0;
  return {
    max: sorted[sorted.length - 1] ?? 0,
    mean: frameMs.reduce((total, ms) => total + ms, 0) / Math.max(1, frameMs.length),
    median: at(0.5),
    p95: at(0.95),
  };
};

const fpsOf = (meanMs: number): number => 1000 / Math.max(meanMs, 1e-9);

test("treasure-chest-pick renders a full click-to-treasure round on Canvas2D at a playable frame rate", () => {
  const headless = installHeadlessCanvas2dBrowser({ cssHeight: CSS_HEIGHT, cssWidth: CSS_WIDTH });
  try {
    // ── the round ────────────────────────────────────────────────────────────
    // A committed outcome supplied from outside, so the reveal — and the work
    // every frame of it does — is the same on every run.
    const source = new InjectedChanceResultSource();
    source.supply(1, { presentationSeed: PRESENTATION_SEED, roundId: "headless#1", tierId: PINNED_TIER, win: true });

    // Where the centre chest is on screen, in the shared logical 960x600 space.
    // This is the coordinate space `pickAt` resolves against, and the space the
    // shell's pointer normalizer (and the capture agent's `__casino.pointer`)
    // feeds — so scripting it here is the player's click, minus the DOM event.
    const target = worldToCanvas(chestCamera(CHEST_COUNT), chestPosition(PICKED_CHEST, CHEST_COUNT));
    assert.notEqual(target, null, "the picked chest projects into frame");
    const { x, y } = target as { readonly x: number; readonly y: number };
    assert.ok(x > 0 && x < CANVAS_WIDTH && y > 0 && y < CANVAS_HEIGHT, `the click lands inside the canvas (${x}, ${y})`);

    let hud: CasinoHud | null = null;

    const running = TREASURE_CHEST_PICK.mount(headless.canvas, {
      backend: "canvas2d",
      config: TREASURE_CHEST_PICK.defaultConfig(),
      onHud: (next): void => {
        hud = next;
      },
      round: 1,
      script: (tick, input): void => {
        // One pointer sample per state change; `InputState` holds the latest
        // until it is cleared, which is exactly how a real cursor behaves.
        if (tick === HOVER_TICK) {
          input.pointerEvent(x, y, false);
        }
        if (tick === PRESS_TICK) {
          input.pointerEvent(x, y, true);
        }
        if (tick === RELEASE_TICK) {
          input.pointerEvent(x, y, false);
        }
      },
      seed: PRESENTATION_SEED,
      // Muted: `casino-mount` then emits no tones at all, so no WebAudio stub is
      // needed and none of the measured time is fake. Nothing else about the
      // presentation changes — particles, shake and every scene node stay on.
      settings: { cameraShake: true, highContrast: false, masterVolume: 0, particleScale: 1, reducedMotion: false, sfxVolume: 0 },
      source,
    });

    try {
      // ── pump ─────────────────────────────────────────────────────────────
      // One animation frame at a time, one fixed simulation step per frame,
      // until the round reaches "complete" — boot, intro, hover, click, the
      // crab's errand, the spiral, the latch, the lid, the treasure, the
      // celebration.
      assert.equal(rendererBackendName(), "Canvas2D", "the round really ran on the software rasterizer");

      const segments = new Map<string, Segment>();
      const all: number[] = [];
      const allNodes: number[] = [];
      const order: string[] = [];

      while (hud === null || hud.phase !== "complete") {
        assert.ok(headless.counters.frames < MAX_FRAMES, `the round completed within ${MAX_FRAMES} frames`);
        const frameMs = headless.frame();
        // The retained scene as this frame left it — an exact, noise-free measure
        // of how much geometry the rasterizer just walked.
        const nodes = rendererNodeCount();
        all.push(frameMs);
        allNodes.push(nodes);
        // Attribute the frame to the phase the game reported DURING it.
        const label = (hud as CasinoHud | null)?.phase ?? "boot";
        let segment = segments.get(label);
        if (segment === undefined) {
          segment = { frameMs: [], label, nodes: [] };
          segments.set(label, segment);
          order.push(label);
        }
        segment.frameMs.push(frameMs);
        segment.nodes.push(nodes);
      }

      // ── the round really happened, and the click is what did it ──────────
      const finished = hud as unknown as CasinoHud;
      assert.equal(finished.phase, "complete");
      assert.equal(finished.win, true, "the injected outcome was revealed as a win");
      assert.equal(finished.tierId, PINNED_TIER, "the treasure revealed is the one committed before the pick");
      assert.equal(
        finished.audit.inputContext?.selectedIndex,
        PICKED_CHEST,
        "the committed outcome resolved against the chest the CLICK landed on",
      );
      assert.equal(finished.audit.commitPhase, "committing", "the outcome was committed in the protected phase");

      // Every phase of the ritual was actually rendered — this is what makes the
      // measurement end-to-end rather than "the idle board, 300 times".
      for (const phase of ["intro", "ready", "committing", "revealing", "celebrating"]) {
        assert.ok((segments.get(phase)?.frameMs.length ?? 0) > 0, `frames were drawn during "${phase}"`);
      }

      // The commit beat and the reveal are long, scripted animations; their frame
      // counts are the timeline constants, which pins that the whole ceremony was
      // watched rather than skipped. One frame of slack each way: the phase change
      // lands mid-frame, so the boundary frame can be attributed to either side.
      const timeline = revealTimeline(1, false);
      const committingFrames = segments.get("committing")?.frameMs.length ?? 0;
      const revealingFrames = segments.get("revealing")?.frameMs.length ?? 0;
      assert.ok(
        Math.abs(committingFrames - commitBeatTicks) <= 1,
        `the crab's approach + the chest's spiral ran their full ${commitBeatTicks} ticks (saw ${committingFrames})`,
      );
      assert.ok(
        Math.abs(revealingFrames - timeline.total) <= 1,
        `the latch/lid/rise ritual ran its full ${timeline.total} ticks (saw ${revealingFrames})`,
      );

      // Every frame presented a framebuffer, and the overlay really drew. A
      // rasterizer that quietly stopped producing pixels would otherwise be the
      // fastest "pass" this test could get.
      assert.equal(headless.counters.blits, headless.counters.frames, "every frame presented exactly one framebuffer");
      assert.ok(headless.counters.overlayOps > 0, "the stylized water overlay drew during the idle board");

      // ── the report ───────────────────────────────────────────────────────
      const backing = headless.backingSize();
      const samples = backing.width * backing.height;
      const overall = summarize(all);
      // Steady state: after the one-off warm-up (framebuffer allocation, every
      // mesh and material created, JIT). This is what a hitch shows up in.
      const steady = summarize(all.slice(WARMUP_FRAMES));
      const peakNodes = Math.max(...allNodes);
      const nodeFrames = allNodes.reduce((total, nodes) => total + nodes, 0);
      const lines = [
        `treasure-chest-pick — end-to-end Canvas2D frame rate (headless)`,
        `  css box      ${CSS_WIDTH}x${CSS_HEIGHT} @ renderScale ${TREASURE_CHEST_PICK.defaultConfig().renderQuality?.renderScale ?? "?"} (fixed-1x)`,
        `  backing      ${backing.width}x${backing.height} = ${(samples / 1000).toFixed(0)}k samples/frame`,
        `  round        ${all.length} frames, click on chest ${PICKED_CHEST} at tick ${RELEASE_TICK}, revealed ${PINNED_TIER}`,
        `  OVERALL      ${fpsOf(overall.mean).toFixed(1)} fps  (mean ${overall.mean.toFixed(2)}ms, median ${overall.median.toFixed(2)}ms, p95 ${overall.p95.toFixed(2)}ms, max ${overall.max.toFixed(2)}ms)`,
        `  fill rate    ${((samples / overall.mean) * (1000 / 1e6)).toFixed(1)} Msamples/s`,
        `  steady       median ${steady.median.toFixed(2)}ms, worst ${steady.max.toFixed(2)}ms (${(steady.max / steady.median).toFixed(1)}x median, hitch limit ${HITCH_LIMIT}x)`,
        `  SCENE COST   peak ${peakNodes} nodes/frame (ceiling ${MAX_NODES_PER_FRAME}), ${nodeFrames} node-frames (ceiling ${MAX_NODE_FRAMES}) — deterministic`,
      ];
      for (const label of order) {
        const segment = segments.get(label) as Segment;
        const stats = summarize(segment.frameMs);
        lines.push(
          `  ${label.padEnd(12)} ${String(segment.frameMs.length).padStart(3)} frames  ${fpsOf(stats.mean).toFixed(1).padStart(5)} fps  (mean ${stats.mean.toFixed(2)}ms, p95 ${stats.p95.toFixed(2)}ms, ${Math.max(...segment.nodes)} nodes peak)`,
        );
      }
      console.log(lines.join("\n"));

      // ── the gate: scene cost (deterministic, so it is the real guard) ────
      // Checked BEFORE the frame-rate floor, because when a change trips both
      // this is the assertion that names the cause instead of the symptom.
      assert.ok(
        peakNodes <= MAX_NODES_PER_FRAME,
        `the busiest frame presented ${peakNodes} scene nodes, over the ${MAX_NODES_PER_FRAME} software-path ceiling — ` +
          `geometry was added to a path that pays ~1 unit of time per node per frame. If this is for the hardware ` +
          `renderer, gate it on the tier (rendererTierAtLeast) rather than raising the ceiling.`,
      );
      assert.ok(
        nodeFrames <= MAX_NODE_FRAMES,
        `the round cost ${nodeFrames} node-frames, over the ${MAX_NODE_FRAMES} ceiling — the scene got heavier ` +
          `across the round even if no single frame broke the per-frame ceiling.`,
      );

      // ── the gate: frame rate ─────────────────────────────────────────────
      assert.ok(
        fpsOf(overall.mean) >= FPS_FLOOR,
        `end-to-end frame rate ${fpsOf(overall.mean).toFixed(1)} fps is below the ${FPS_FLOOR} fps floor (mean frame ${overall.mean.toFixed(2)}ms over ${all.length} frames)`,
      );

      // The reveal is the most expensive stretch (hero-scale chest, the veil, the
      // treasure, the burst motes), and it is also the moment the player is
      // watching most closely. Hold it to the same floor rather than letting a
      // cheap idle board average it back up.
      const reveal = segments.get("revealing") as Segment;
      const revealStats = summarize(reveal.frameMs);
      assert.ok(
        fpsOf(revealStats.mean) >= FPS_FLOOR,
        `the reveal ran at ${fpsOf(revealStats.mean).toFixed(1)} fps, below the ${FPS_FLOOR} fps floor (mean frame ${revealStats.mean.toFixed(2)}ms)`,
      );

      // Frame-time consistency: a single frame an order of magnitude over the
      // median is a visible hitch, and the usual cause is a per-frame allocation
      // large enough to trip a GC pause. Measured on the steady state only — the
      // warm-up legitimately pays for the framebuffer and every mesh, once.
      assert.ok(
        steady.max <= steady.median * HITCH_LIMIT,
        `worst steady-state frame ${steady.max.toFixed(2)}ms is more than ${HITCH_LIMIT}x the median ${steady.median.toFixed(2)}ms — a per-frame allocation or GC hitch`,
      );

      // Sanity on the harness itself: the whole run drew the framebuffer it says
      // it did, at the resolution the quality asks for.
      assert.equal(headless.counters.blitPixels, samples * all.length, "every frame presented a full-resolution framebuffer");
      assert.ok(CHEST_TIMING.spiralTicks > 0, "the spiral has a duration (guards the timeline constants this test reads)");
    } finally {
      running.stop();
    }
  } finally {
    headless.teardown();
  }
});
