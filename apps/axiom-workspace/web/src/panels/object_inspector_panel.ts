// Object Inspector panel (id: "object-inspector", region: right).
//
// Shows the selected object's placeholder fields (name / value rows). Selection
// itself flows through the typed `object.placeholder.select` event; here we also
// expose a placeholder "select" button so the flow is demonstrable.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";

export function renderObjectInspectorPanel(
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "object-inspector";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Object Inspector";

  const selected = document.createElement("p");
  selected.className = "ws-panel-note";
  selected.textContent = `selected: ${state.objectInspector.selectedObjectId ?? "none (placeholder)"}`;

  const select = document.createElement("button");
  select.type = "button";
  select.className = "ws-inline-button";
  select.textContent = "Select Placeholder Object";
  select.addEventListener("click", () => {
    dispatch({
      type: "object.placeholder.select",
      objectId: "object.placeholder.01",
    });
  });

  const dl = document.createElement("dl");
  dl.className = "ws-field-list";
  state.objectInspector.fields.forEach((field) => {
    const dt = document.createElement("dt");
    dt.className = "ws-field-label";
    dt.textContent = field.name;
    const dd = document.createElement("dd");
    dd.className = "ws-field-value";
    dd.textContent = field.value;
    dl.append(dt, dd);
  });

  section.append(bar, selected, select, dl);
  return section;
}
