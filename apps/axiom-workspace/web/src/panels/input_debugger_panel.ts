// Input Debugger panel (id: "input-debugger", region: bottom).
//
// A read-only, data-driven list of captured input events (tick / code / label).
// No runtime is attached, so it stays empty until real input is captured.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";
import { renderEmpty } from "./empty_state";

export function renderInputDebuggerPanel(
  state: WorkspaceBrowserState,
  _dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "input-debugger";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Input Debugger";

  const inputs = state.inputDebugger.inputs;
  if (inputs.length === 0) {
    section.append(bar, renderEmpty("No input captured"));
    return section;
  }

  const list = document.createElement("ul");
  list.className = "ws-input-list";
  inputs.forEach((input) => {
    const item = document.createElement("li");
    item.className = "ws-input-row";

    const tick = document.createElement("span");
    tick.className = "ws-input-tick";
    tick.textContent = `t${input.tick}`;
    const code = document.createElement("span");
    code.className = "ws-input-code";
    code.textContent = `0x${input.code.toString(16)}`;
    const label = document.createElement("span");
    label.className = "ws-input-label";
    label.textContent = input.label;

    item.append(tick, code, label);
    list.append(item);
  });

  section.append(bar, list);
  return section;
}
