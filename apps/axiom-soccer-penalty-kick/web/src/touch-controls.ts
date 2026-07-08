/*
 * On-screen touch controls for the penalty game — a mobile-friendly control layer
 * the game mounts into the page and folds into its input intent. Three widgets:
 *
 *   • an analog AIM joystick (the knob follows the finger; its offset is the aim),
 *   • a SHOOT button (hold to charge power, release to shoot — and, harmlessly,
 *     the same release continues between rounds), and
 *   • a RESET button (start the shootout over).
 *
 * It is pointer-event driven, so it works with touch AND mouse, and has no engine
 * coupling: it only mutates a shared input-state singleton the game reads once per
 * fixed tick (the one-shot edges are consumed on read).
 */

interface TouchState {
  aimX: number; // analog aim, -100..100 (right = +)
  aimY: number; // analog aim, -100..100 (up = +)
  shootHeld: boolean; // SHOOT held this instant (charging power)
  shootReleased: boolean; // one-shot: SHOOT released since the last read
  resetPulse: boolean; // one-shot: RESET tapped since the last read
}

const state: TouchState = { aimX: 0, aimY: 0, shootHeld: false, shootReleased: false, resetPulse: false };

/** The per-tick touch input the game reads; the one-shot edges are consumed on read. */
export const touchInput = {
  get aimX(): number {
    return state.aimX;
  },
  get aimY(): number {
    return state.aimY;
  },
  get shootHeld(): boolean {
    return state.shootHeld;
  },
  takeShootReleased(): boolean {
    const fired = state.shootReleased;
    state.shootReleased = false;
    return fired;
  },
  takeReset(): boolean {
    const fired = state.resetPulse;
    state.resetPulse = false;
    return fired;
  },
};

const clamp = (v: number, lo: number, hi: number): number => Math.min(Math.max(v, lo), hi);

// How far (px) the knob travels from center for full deflection. Kept in sync with
// the joystick base / knob sizing in index.html.
const JOYSTICK_RADIUS = 40;

/**
 * Wire the AIM joystick: while a pointer is down on the base, the knob tracks it
 * (capped to the base radius) and the normalized offset becomes the analog aim
 * (screen-up = aim-up). Recenters and zeroes the aim on release.
 */
const wireJoystick = (base: HTMLElement, knob: HTMLElement): void => {
  let activePointer = -1;

  const track = (event: PointerEvent): void => {
    const rect = base.getBoundingClientRect();
    const dx = event.clientX - (rect.left + rect.width / 2);
    const dy = event.clientY - (rect.top + rect.height / 2);
    const dist = Math.hypot(dx, dy) || 1;
    const capped = Math.min(dist, JOYSTICK_RADIUS);
    const nx = (dx / dist) * capped;
    const ny = (dy / dist) * capped;
    knob.style.transform = `translate(${nx}px, ${ny}px)`;
    state.aimX = clamp((nx / JOYSTICK_RADIUS) * 100, -100, 100);
    state.aimY = clamp((-ny / JOYSTICK_RADIUS) * 100, -100, 100);
  };

  const recenter = (event: PointerEvent): void => {
    if (event.pointerId !== activePointer) return;
    activePointer = -1;
    knob.style.transform = "translate(0px, 0px)";
    state.aimX = 0;
    state.aimY = 0;
  };

  base.addEventListener("pointerdown", (event: PointerEvent): void => {
    activePointer = event.pointerId;
    track(event);
    try {
      base.setPointerCapture(event.pointerId);
    } catch {
      // Some pointer types / environments can't be captured; tracking still works.
    }
    event.preventDefault();
  });
  base.addEventListener("pointermove", (event: PointerEvent): void => {
    if (event.pointerId === activePointer) track(event);
  });
  base.addEventListener("pointerup", recenter);
  base.addEventListener("pointercancel", recenter);
};

/**
 * Wire the SHOOT button: held = charging; release = shoot. The release also raises
 * a one-shot the game maps to "continue", so the same button advances the prompt
 * between rounds (a stray continue while aiming is ignored by the sim).
 */
const wireShoot = (button: HTMLElement): void => {
  button.addEventListener("pointerdown", (event: PointerEvent): void => {
    state.shootHeld = true;
    try {
      button.setPointerCapture(event.pointerId);
    } catch {
      // Capture is best-effort; the pointerup/cancel release still fires.
    }
    event.preventDefault();
  });
  const release = (): void => {
    if (!state.shootHeld) return;
    state.shootHeld = false;
    state.shootReleased = true;
  };
  button.addEventListener("pointerup", release);
  button.addEventListener("pointercancel", release);
};

/** Wire the RESET button: a tap pulses a reset request (start the shootout over). */
const wireReset = (button: HTMLElement): void => {
  button.addEventListener("pointerdown", (event: PointerEvent): void => {
    state.resetPulse = true;
    event.preventDefault();
  });
};

/**
 * Mount the on-screen controls by wiring the markup already in the page
 * (#joystick / #joystick-knob / #shoot-btn / #reset-btn). Called once by the game
 * on its first tick, after the DOM is ready.
 */
export const mountTouchControls = (): void => {
  const byId = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;
  wireJoystick(byId("joystick"), byId("joystick-knob"));
  wireShoot(byId("shoot-btn"));
  wireReset(byId("reset-btn"));
};
