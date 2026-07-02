// DOM mounting for the workspace shell.
//
// Given a root element, an initial state, and a dispatch callback, this builds
// the region layout (left column, center viewport, right column, bottom bar),
// places each panel from `panel_registry` in `WORKSPACE_LAYOUT` order and region,
// and re-renders on every dispatch. The dispatch loop is explicit and typed: it
// applies the event through the single reducer, stores the new state, and rebuilds
// the DOM. No global mutation beyond the closed-over `current` state cell.

import { renderPanel } from "./panel_registry";
import {
  applyWorkspaceEvent,
  type Dispatch,
  type WorkspaceEvent,
} from "./workspace_events";
import { WORKSPACE_LAYOUT, type WorkspaceRegion } from "./workspace_layout";
import type { WorkspaceBrowserState } from "./workspace_state";

const REGION_LABEL: Readonly<Record<WorkspaceRegion, string>> = {
  left: "Authoring",
  center: "Viewport",
  right: "Inspect / Export",
  bottom: "Observe / Control",
};

function buildRegion(
  region: WorkspaceRegion,
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
): HTMLElement {
  const column = document.createElement("div");
  column.className = "ws-region";
  column.dataset.region = region;

  const heading = document.createElement("div");
  heading.className = "ws-region-heading";
  heading.textContent = REGION_LABEL[region];
  column.append(heading);

  WORKSPACE_LAYOUT.filter((entry) => entry.region === region).forEach((entry) => {
    column.append(renderPanel(entry.id, state, dispatch));
  });
  return column;
}

function build(
  root: HTMLElement,
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
): void {
  root.replaceChildren();

  const main = document.createElement("div");
  main.className = "ws-main";

  const columns = document.createElement("div");
  columns.className = "ws-columns";
  ["left", "center", "right"].forEach((region) => {
    columns.append(buildRegion(region as WorkspaceRegion, state, dispatch));
  });

  const bottom = buildRegion("bottom", state, dispatch);
  bottom.classList.add("ws-bottom");

  main.append(columns, bottom);
  root.append(main);
}

export function mountWorkspace(
  root: HTMLElement,
  initial: WorkspaceBrowserState,
): void {
  const current: { value: WorkspaceBrowserState } = { value: initial };

  const dispatch: Dispatch = (event: WorkspaceEvent): void => {
    current.value = applyWorkspaceEvent(current.value, event);
    build(root, current.value, dispatch);
  };

  build(root, current.value, dispatch);
}
