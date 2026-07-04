// Package / Export panel (id: "package-export", region: right).
//
// A read-only view of the current build/export target and status. No runtime is
// attached, so nothing is configured and it stays empty until an export is set up.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";
import { UNSET } from "../workspace_state";
import { renderEmpty } from "./empty_state";

export function renderPackageExportPanel(
  state: WorkspaceBrowserState,
  _dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "package-export";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Package / Export";

  const pkg = state.packageExport;
  if (pkg.target === UNSET && pkg.status === UNSET) {
    section.append(bar, renderEmpty("No export configured"));
    return section;
  }

  const dl = document.createElement("dl");
  dl.className = "ws-field-list";
  const rows: readonly (readonly [string, string])[] = [
    ["status", pkg.status],
    ["target", pkg.target],
    ["requested", String(pkg.requested)],
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

  section.append(bar, dl);
  return section;
}
