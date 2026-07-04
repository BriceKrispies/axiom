// Object Inspector panel (id: "object-inspector", region: right).
//
// A read-only view of the selected object's fields (name / value). No runtime is
// attached, so nothing is selected and it stays empty until a real object arrives.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";
import { renderEmpty } from "./empty_state";

export function renderObjectInspectorPanel(
  state: WorkspaceBrowserState,
  _dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "object-inspector";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Object Inspector";

  const fields = state.objectInspector.fields;
  if (fields.length === 0) {
    section.append(bar, renderEmpty("No object selected"));
    return section;
  }

  const dl = document.createElement("dl");
  dl.className = "ws-field-list";
  fields.forEach((field) => {
    const dt = document.createElement("dt");
    dt.className = "ws-field-label";
    dt.textContent = field.name;
    const dd = document.createElement("dd");
    dd.className = "ws-field-value";
    dd.textContent = field.value;
    dl.append(dt, dd);
  });

  section.append(bar, dl);
  return section;
}
