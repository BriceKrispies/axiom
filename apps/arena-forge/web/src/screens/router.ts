/*
 * router.ts — the centralized screen state machine. It owns exactly one active
 * `Screen` and performs every transition as `exit()` (old) → build (new) →
 * `enter()` (new), so navigation is never a scattered set of booleans and
 * lifecycle is deterministic. It is DOM-free and engine-free (it only touches the
 * `Screen` interface), so the state machine is unit-testable under `node --test`
 * with fake screens. The app shell (`../game.ts`) constructs it with a factory
 * that builds the real menu / gameplay / figure-lab screens.
 */

import type { Screen, ScreenNav, ScreenState } from "./screen.ts";

export class ScreenRouter implements ScreenNav {
  private currentScreen: Screen;
  private currentState: ScreenState;
  private readonly factory: (state: ScreenState, nav: ScreenNav) => Screen;

  public constructor(factory: (state: ScreenState, nav: ScreenNav) => Screen, initial: ScreenState) {
    this.factory = factory;
    this.currentState = initial;
    this.currentScreen = factory(initial, this);
    this.currentScreen.enter();
  }

  public get state(): ScreenState {
    return this.currentState;
  }

  public get screen(): Screen {
    return this.currentScreen;
  }

  public goto(state: ScreenState): void {
    if (state === this.currentState) {
      return;
    }
    this.currentScreen.exit();
    this.currentState = state;
    this.currentScreen = this.factory(state, this);
    this.currentScreen.enter();
  }

  public update(): void {
    this.currentScreen.update();
  }

  public renderScene3D(): void {
    this.currentScreen.renderScene3D();
  }

  public render(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    this.currentScreen.render(ctx, w, h);
  }

  public onPointerDown(x: number, y: number): void {
    this.currentScreen.onPointerDown(x, y);
  }

  public onPointerMove(x: number, y: number): void {
    this.currentScreen.onPointerMove(x, y);
  }

  public onPointerUp(x: number, y: number): void {
    this.currentScreen.onPointerUp(x, y);
  }

  public onWheel(deltaY: number): void {
    this.currentScreen.onWheel?.(deltaY);
  }

  public onPinch(factor: number): void {
    this.currentScreen.onPinch?.(factor);
  }
}
