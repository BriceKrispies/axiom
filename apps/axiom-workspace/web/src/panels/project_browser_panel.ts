// Project Browser panel (id: "project-browser", region: left).
//
// Lists placeholder projects. Clicking a row dispatches a typed
// `project.placeholder.open` event through the shared reducer.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";

export function renderProjectBrowserPanel(
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "project-browser";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Project Browser";

  const note = document.createElement("p");
  note.className = "ws-panel-note";
  note.textContent = "Placeholder projects — click a row to open (shell state only).";

  const list = document.createElement("ul");
  list.className = "ws-row-list";

  state.projectBrowser.projects.forEach((project, index) => {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ws-row-button";
    const selected = state.projectBrowser.selectedIndex === index;
    button.setAttribute("aria-pressed", selected ? "true" : "false");
    button.dataset.selected = selected ? "true" : "false";

    const id = document.createElement("span");
    id.className = "ws-row-id";
    id.textContent = project.id;
    const name = document.createElement("span");
    name.className = "ws-row-name";
    name.textContent = project.name;

    button.append(id, name);
    button.addEventListener("click", () => {
      dispatch({ type: "project.placeholder.open", projectId: project.id });
    });
    item.append(button);
    list.append(item);
  });

  section.append(bar, note, list);
  return section;
}
