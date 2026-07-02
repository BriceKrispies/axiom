// Input Debugger panel (id: "input-debugger", region: bottom).
//
// Shows placeholder input events (tick / code / label) in insertion order, plus a
// placeholder "record" button that dispatches a typed `input.placeholder.record`
// event. The button updates shell state only — it does not read real input.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";

export function renderInputDebuggerPanel(
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "input-debugger";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Input Debugger";

  const record = document.createElement("button");
  record.type = "button";
  record.className = "ws-inline-button";
  record.textContent = "Record Placeholder Input";
  record.addEventListener("click", () => {
    dispatch({
      type: "input.placeholder.record",
      code: 0x40,
      label: "placeholder.recorded",
    });
  });

  const list = document.createElement("ul");
  list.className = "ws-input-list";

  state.inputDebugger.inputs.forEach((input) => {
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

  section.append(bar, record, list);
  return section;
}
