// Project Browser panel (id: "project-browser", region: left).
//
// A read-only, data-driven list of known/openable game projects. Empty (no
// placeholder projects) until a real project source is wired in.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";
import { renderEmpty } from "./empty_state";

export function renderProjectBrowserPanel(
  state: WorkspaceBrowserState,
  _dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "project-browser";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Project Browser";

  const projects = state.projectBrowser.projects;
  if (projects.length === 0) {
    section.append(bar, renderEmpty("No projects"));
    return section;
  }

  const list = document.createElement("ul");
  list.className = "ws-row-list";
  projects.forEach((project, index) => {
    const item = document.createElement("li");
    item.className = "ws-row";
    item.dataset.selected = state.projectBrowser.selectedIndex === index ? "true" : "false";

    const id = document.createElement("span");
    id.className = "ws-row-id";
    id.textContent = project.id;
    const name = document.createElement("span");
    name.className = "ws-row-name";
    name.textContent = project.name;

    item.append(id, name);
    list.append(item);
  });

  section.append(bar, list);
  return section;
}
