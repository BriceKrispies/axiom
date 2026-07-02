// Play Controls panel (id: "play-controls", region: bottom).
//
// Placeholder workflow-mode buttons (Edit / Play / Drop In / Replay / Package).
// Each dispatches a typed `workspace.mode.set` event with the matching mode; the
// Drop In button additionally dispatches `drop_in.placeholder.create`, and Play
// additionally dispatches `launch.placeholder.create`. These buttons update SHELL
// state only — they never simulate a runtime. The current mode is shown.

import type { Dispatch, WorkspaceEvent } from "../workspace_events";
import type { WorkspaceBrowserState, WorkspaceMode } from "../workspace_state";

interface ModeButtonSpec {
  readonly mode: WorkspaceMode;
  readonly label: string;
  readonly extra: readonly WorkspaceEvent[];
}

const MODE_BUTTONS: readonly ModeButtonSpec[] = [
  { mode: "Edit", label: "Edit", extra: [] },
  { mode: "Play", label: "Play", extra: [{ type: "launch.placeholder.create" }] },
  {
    mode: "DropIn",
    label: "Drop In",
    extra: [{ type: "drop_in.placeholder.create", levelId: "level.placeholder.01" }],
  },
  { mode: "Replay", label: "Replay", extra: [] },
  { mode: "Package", label: "Package", extra: [] },
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
  status.textContent = `mode: ${state.mode} · launch: ${String(state.playControls.launchCreated)} · dropIn: ${String(state.playControls.dropInCreated)}`;

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
      spec.extra.forEach((event) => {
        dispatch(event);
      });
    });
    row.append(button);
  });

  section.append(bar, status, row);
  return section;
}
