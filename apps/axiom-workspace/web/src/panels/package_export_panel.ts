// Package / Export panel (id: "package-export", region: right).
//
// Shows placeholder build/export status and target, plus a placeholder "request"
// button that dispatches a typed `package.placeholder.request` event. This only
// updates shell state; it does not run a real packaging pipeline.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";

export function renderPackageExportPanel(
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "package-export";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Package / Export";

  const dl = document.createElement("dl");
  dl.className = "ws-field-list";
  const rows: readonly (readonly [string, string])[] = [
    ["status", state.packageExport.status],
    ["target", state.packageExport.target],
    ["requested", String(state.packageExport.requested)],
  ];
  rows.forEach(([label, value]) => {
    const dt = document.createElement("dt");
    dt.className = "ws-field-label";
    dt.textContent = label;
    const dd = document.createElement("dd");
    dd.className = "ws-field-value";
    dd.textContent = value;
    dl.append(dt, dd);
  });

  const request = document.createElement("button");
  request.type = "button";
  request.className = "ws-inline-button";
  request.textContent = "Request Placeholder Package";
  request.addEventListener("click", () => {
    dispatch({
      type: "package.placeholder.request",
      target: "browser (placeholder)",
    });
  });

  section.append(bar, dl, request);
  return section;
}
