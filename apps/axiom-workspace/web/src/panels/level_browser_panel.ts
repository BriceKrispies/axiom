// Level Browser panel (id: "level-browser", region: left).
//
// Lists placeholder levels. Selecting a row dispatches a typed
// `level.placeholder.select` event through the shared reducer.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";

export function renderLevelBrowserPanel(
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "level-browser";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Level Browser";

  const note = document.createElement("p");
  note.className = "ws-panel-note";
  note.textContent = "Placeholder levels — click a row to select (shell state only).";

  const list = document.createElement("ul");
  list.className = "ws-row-list";

  state.levelBrowser.levels.forEach((level, index) => {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ws-row-button";
    const selected = state.levelBrowser.selectedIndex === index;
    button.setAttribute("aria-pressed", selected ? "true" : "false");
    button.dataset.selected = selected ? "true" : "false";

    const id = document.createElement("span");
    id.className = "ws-row-id";
    id.textContent = level.id;
    const name = document.createElement("span");
    name.className = "ws-row-name";
    name.textContent = level.name;

    button.append(id, name);
    button.addEventListener("click", () => {
      dispatch({ type: "level.placeholder.select", index });
    });
    item.append(button);
    list.append(item);
  });

  section.append(bar, note, list);
  return section;
}
