import { strict as assert } from "node:assert";
import { test } from "node:test";

import { ScreenRouter } from "./router.ts";
import type { Screen, ScreenNav, ScreenState } from "./screen.ts";

class FakeScreen implements Screen {
  public entered = 0;
  public exited = 0;
  public readonly name: ScreenState;
  private readonly log: string[];
  public constructor(name: ScreenState, log: string[]) {
    this.name = name;
    this.log = log;
  }
  public enter(): void {
    this.entered += 1;
    this.log.push(`enter:${this.name}`);
  }
  public exit(): void {
    this.exited += 1;
    this.log.push(`exit:${this.name}`);
  }
  public update(): void {}
  public renderScene3D(): void {}
  public render(): void {}
  public onPointerDown(): void {}
  public onPointerMove(): void {}
  public onPointerUp(): void {}
}

const makeRouter = (): { router: ScreenRouter; log: string[]; built: FakeScreen[] } => {
  const log: string[] = [];
  const built: FakeScreen[] = [];
  const factory = (state: ScreenState, _nav: ScreenNav): FakeScreen => {
    const s = new FakeScreen(state, log);
    built.push(s);
    return s;
  };
  return { router: new ScreenRouter(factory, "main_menu"), log, built };
};

test("the app starts on the main menu and enters it once", () => {
  const { router, log } = makeRouter();
  assert.equal(router.state, "main_menu");
  assert.deepEqual(log, ["enter:main_menu"]);
});

test("goto exits the old screen then enters the new one, in order", () => {
  const { router, log } = makeRouter();
  router.goto("gameplay");
  assert.equal(router.state, "gameplay");
  assert.deepEqual(log, ["enter:main_menu", "exit:main_menu", "enter:gameplay"]);
  router.goto("figure_lab");
  assert.equal(router.state, "figure_lab");
  assert.deepEqual(log.slice(-2), ["exit:gameplay", "enter:figure_lab"]);
});

test("back from gameplay and from figure lab returns to the main menu", () => {
  const { router } = makeRouter();
  router.goto("gameplay");
  router.goto("main_menu");
  assert.equal(router.state, "main_menu");
  router.goto("figure_lab");
  router.goto("main_menu");
  assert.equal(router.state, "main_menu");
});

test("goto to the current state is a no-op (no re-enter/exit)", () => {
  const { router, log } = makeRouter();
  const before = log.length;
  router.goto("main_menu");
  assert.equal(log.length, before);
});

test("repeated transitions keep enter/exit balanced — no leaked screens", () => {
  const { router, built } = makeRouter();
  const seq: ScreenState[] = ["gameplay", "main_menu", "figure_lab", "main_menu", "gameplay", "main_menu"];
  for (const s of seq) {
    router.goto(s);
  }
  // Every screen except the active one must have been exited exactly as many
  // times as it was entered (each built screen enters once).
  for (const s of built) {
    assert.equal(s.entered, 1, `${s.name} entered ${s.entered} times`);
    const isActive = s === (router.screen as unknown as FakeScreen);
    assert.equal(s.exited, isActive ? 0 : 1, `${s.name} exited ${s.exited} times`);
  }
});
