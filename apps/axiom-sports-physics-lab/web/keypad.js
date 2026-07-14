// On-screen touch keypad — pure JS overlay. It dispatches synthetic
// KeyboardEvents on `window`, which is exactly what the wasm apps already listen
// for (e.g. netplay's `web.rs` reads `ArrowLeft/Right/Up/Down` keydown/keyup on
// the window). So the keypad drives the demo with zero changes to engine/app
// input code — it is a presentation-only shim living in the gallery shell.

/**
 * Render a keypad of `buttons` into `container`. Each button is
 * `{ key, label, pos }`, where `key` is the DOM KeyboardEvent key the app reads
 * (e.g. "ArrowUp") and `pos` places it in the D-pad grid (up/down/left/right).
 * No-op styling-wise when `buttons` is empty (caller skips that case).
 */
export function renderKeypad(container, buttons) {
  container.classList.add("keypad");
  for (const b of buttons) {
    const el = document.createElement("button");
    el.type = "button";
    el.className = "pad" + (b.pos ? " pad-" + b.pos : "");
    el.textContent = b.label;
    el.setAttribute("aria-label", b.key);
    bindButton(el, b.key);
    container.appendChild(el);
  }
}

// Wire one button to press/release the given key. Uses pointer events so it
// works for both touch and mouse, guards against duplicate keydowns while held,
// and releases on pointerup/leave/cancel so a key never sticks.
function bindButton(el, key) {
  let held = false;
  const press = (e) => {
    e.preventDefault();
    if (held) return;
    held = true;
    el.classList.add("active");
    if (el.setPointerCapture && e.pointerId != null) {
      try {
        el.setPointerCapture(e.pointerId);
      } catch {
        // Capture is best-effort; release still fires on pointerup/leave.
      }
    }
    dispatch("keydown", key);
  };
  const release = (e) => {
    if (e) e.preventDefault();
    if (!held) return;
    held = false;
    el.classList.remove("active");
    dispatch("keyup", key);
  };
  el.addEventListener("pointerdown", press);
  el.addEventListener("pointerup", release);
  el.addEventListener("pointercancel", release);
  el.addEventListener("pointerleave", release);
  el.addEventListener("contextmenu", (e) => e.preventDefault());
}

function dispatch(type, key) {
  window.dispatchEvent(new KeyboardEvent(type, { key, bubbles: true }));
}
