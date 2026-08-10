/*
 * The browser boot harness for Sky Drop — the host / platform edge (NOT engine spine),
 * so it lives in the app `web/` dir, outside the branchless + coverage gates, and uses
 * ordinary control flow. `createGame()` mints the per-game registry the author module
 * (`game.ts`) registers its `onFixedUpdate` into; the SDK's `boot()` builds the
 * deterministic loop over the wasm bridge, wires DOM input, and presents the authored
 * 3D world every frame. This harness adds the DOM HUD: the wind readout, the
 * altimeter, the drop counter, the landing verdict, and the round-over card.
 *
 * The three dev-server couplings (the wasm init call, the versioned hot-reload import,
 * and the `/events` SSE channel) are the anchors the single-file packager rewrites for
 * a static build — keep them verbatim.
 */

import { boot } from "/vendor/axiom-game/boot.js";
import { createGame, onRender } from "@axiom/game";

import initWasm, { WasmGame } from "/pkg/axiom_game_runtime.js";

const FIXED_HZ = 60;
const SEED = 1n;
const NANOS_PER_SECOND = 1_000_000_000;
const FIXED_STEP_NANOS = Math.round(NANOS_PER_SECOND / FIXED_HZ);
const MAX_STEPS_PER_FRAME = 8;
const MAX_INSTANCES = 4096;
const CANVAS_ID = "axiom-canvas";
/** Below this drift speed the wind is not worth compensating — say so plainly. */
const CALM_WIND = 0.35;

interface HudThrow {
  readonly index: number;
  readonly label: string;
  readonly distance: number;
  readonly points: number;
  readonly onTarget: boolean;
}

interface HudScoreboard {
  readonly total: number;
  readonly best: number;
  readonly isRecord: boolean;
  readonly bullseyes: number;
  readonly onTarget: number;
  readonly tightest: number | null;
  readonly throws: readonly HudThrow[];
}

/** The HUD snapshot the game module exposes each frame. */
interface Hud {
  readonly phase: "throwing" | "settling" | "results";
  readonly ball: number;
  readonly ballsTotal: number;
  readonly ballsLeft: number;
  readonly inFlight: number;
  readonly windSpeed: number;
  readonly windAngle: number;
  readonly standDistance: number;
  readonly holding: boolean;
  /** Populated ONLY once every ball is down — see `round.ts`. */
  readonly scoreboard: HudScoreboard | null;
}

interface SkyDropModule {
  readonly readHud: () => Hud;
  readonly configureViewport: (width: number, height: number) => void;
}

const el = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;

const boot_ = async (): Promise<void> => {
  const canvas = el("axiom-canvas") as HTMLCanvasElement;
  const gameover = el("gameover");
  const alt = el("alt");
  const windArrow = el("wind-arrow");
  const fields = {
    altValue: el("alt-value"),
    best: el("best"),
    drop: el("drop"),
    goBest: el("go-best"),
    goBulls: el("go-bulls"),
    goRows: el("go-rows"),
    goScore: el("go-score"),
    stand: el("stand"),
    windSpeed: el("wind-speed"),
  };

  await initWasm();

  // Pointer events arrive in DISPLAYED (CSS) pixels relative to the canvas, and the
  // toss normalises the gesture by the canvas height — so the game must be told the
  // canvas's *displayed* size, not its 720×600 backing, or an identical swipe throws a
  // different distance on a scaled-down (mobile) canvas. Keep it fresh across resizes.
  let applyViewport = (): void => {};
  globalThis.addEventListener("resize", (): void => applyViewport());
  globalThis.addEventListener("orientationchange", (): void => applyViewport());

  /** Build the per-throw breakdown once, when the rack comes down. */
  const renderScoreboard = (board: HudScoreboard): void => {
    fields.goRows.textContent = "";
    for (const shot of board.throws) {
      const row = document.createElement("tr");
      row.className = shot.onTarget ? "" : "miss";
      const cell = (text: string, cls = ""): HTMLTableCellElement => {
        const td = document.createElement("td");
        td.className = cls;
        td.textContent = text;
        return td;
      };
      row.append(
        cell(`${shot.index + 1}.`, "label"),
        cell(shot.label, "label"),
        cell(`${shot.distance.toFixed(2)} m`),
        cell(`+${shot.points}`, "pts"),
      );
      fields.goRows.append(row);
    }
    fields.goScore.textContent = String(board.total);
    fields.goBest.textContent = String(board.best);
    fields.goBest.classList.toggle("record", board.isRecord);
    const tightest = board.tightest === null ? "—" : `${board.tightest.toFixed(2)} m`;
    fields.goBulls.textContent =
      `${board.bullseyes} bullseye${board.bullseyes === 1 ? "" : "s"} · ` +
      `${board.onTarget}/${board.throws.length} on target · best ${tightest}`;
  };

  // The scoreboard is rebuilt only on the transition into "results", never per frame.
  let scoreboardShown = false;

  const updateHud = (hud: Hud): void => {
    fields.drop.textContent = `${hud.ball}/${hud.ballsTotal}`;
    fields.stand.textContent = `${hud.standDistance.toFixed(0)} m`;

    // Wind: the arrow points the way the wind PUSHES, in screen space (0° = up-screen,
    // the direction the camera faces down the stand→target line).
    const calm = hud.windSpeed < CALM_WIND;
    fields.windSpeed.textContent = calm ? "CALM" : `${hud.windSpeed.toFixed(1)} m/s`;
    fields.windSpeed.classList.toggle("wind-calm", calm);
    windArrow.style.transform = `rotate(${hud.windAngle.toFixed(1)}deg)`;
    windArrow.style.opacity = calm ? "0.35" : "1";

    // Balls left in the rack. This is the ONLY running counter on screen — no score,
    // no per-throw verdict. See `round.ts`.
    fields.altValue.textContent = String(hud.ballsLeft);
    alt.classList.toggle("spent", hud.ballsLeft === 0);

    const over = hud.phase === "results" && hud.scoreboard !== null;
    gameover.classList.toggle("show", over);
    if (over && !scoreboardShown && hud.scoreboard !== null) {
      renderScoreboard(hud.scoreboard);
      fields.best.textContent = String(hud.scoreboard.best);
    }
    scoreboardShown = over;
  };

  let teardown: (() => void) | undefined;
  const load = async (version: number): Promise<void> => {
    teardown?.();
    const game = new WasmGame(FIXED_STEP_NANOS, MAX_STEPS_PER_FRAME);
    const app = createGame({ fixedHz: FIXED_HZ, seed: SEED, surface: CANVAS_ID });
    const mod = (await import(`/dist/game.js?v=${version}`)) as SkyDropModule;
    applyViewport = (): void => mod.configureViewport(canvas.clientWidth || canvas.width, canvas.clientHeight || canvas.height);
    applyViewport();

    onRender((): void => updateHud(mod.readHud()));

    app.start();
    // frameLocked: one sim tick per displayed frame, so the first frame builds the
    // whole scene (registering every material) BEFORE the 3D surface binds.
    teardown = boot(game as unknown as Parameters<typeof boot>[0], app, {
      canvas,
      frameLocked: true,
      present3d: { maxInstances: MAX_INSTANCES },
    });
  };

  await load(0);

  const isDev = location.hostname === "localhost" || location.hostname === "127.0.0.1";
  if (isDev) {
    const events = new EventSource("/events");
    events.addEventListener("reload", (event: MessageEvent<string>): void => {
      void load(Number(event.data));
    });
  }
};

void boot_();
