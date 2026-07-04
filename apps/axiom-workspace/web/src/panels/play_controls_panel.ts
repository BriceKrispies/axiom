// Play Controls panel (id: "play-controls", region: bottom).
//
// The workflow-mode switcher (Edit / Play / Drop In / Replay / Package). Each
// button dispatches a typed `workspace.mode.set` event with the matching mode —
// the one genuine control in the shell (it drives which workflow the layout
// presents). It attaches no runtime and produces no placeholder data.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState, WorkspaceMode } from "../workspace_state";

interface ModeButtonSpec {
  readonly mode: WorkspaceMode;
  readonly label: string;
}

const MODE_BUTTONS: readonly ModeButtonSpec[] = [
  { mode: "Edit", label: "Edit" },
  { mode: "Play", label: "Play" },
  { mode: "DropIn", label: "Drop In" },
  { mode: "Replay", label: "Replay" },
  { mode: "Package", label: "Package" },
];

export function renderPlayControlsPanel(
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "play-controls";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Play Controls";

  const status = document.createElement("p");
  status.className = "ws-panel-note";
  status.textContent = `mode: ${state.mode}`;

  const row = document.createElement("div");
  row.className = "ws-button-row";

  MODE_BUTTONS.forEach((spec) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ws-mode-button";
    button.textContent = spec.label;
    const active = state.mode === spec.mode;
    button.dataset.active = active ? "true" : "false";
    button.setAttribute("aria-pressed", active ? "true" : "false");
    button.addEventListener("click", () => {
      dispatch({ type: "workspace.mode.set", mode: spec.mode });
    });
    row.append(button);
  });

  section.append(bar, status, row);
  return section;
}
