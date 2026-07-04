// Level Browser panel (id: "level-browser", region: left).
//
// A read-only, data-driven list of the opened project's levels. No runtime is
// attached, so it stays empty (no placeholder levels) until real levels arrive.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";
import { renderEmpty } from "./empty_state";

export function renderLevelBrowserPanel(
  state: WorkspaceBrowserState,
  _dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "level-browser";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Level Browser";

  const levels = state.levelBrowser.levels;
  if (levels.length === 0) {
    section.append(bar, renderEmpty("No levels"));
    return section;
  }

  const list = document.createElement("ul");
  list.className = "ws-row-list";
  levels.forEach((level, index) => {
    const item = document.createElement("li");
    item.className = "ws-row";
    item.dataset.selected = state.levelBrowser.selectedIndex === index ? "true" : "false";

    const id = document.createElement("span");
    id.className = "ws-row-id";
    id.textContent = level.id;
    const name = document.createElement("span");
    name.className = "ws-row-name";
    name.textContent = level.name;

    item.append(id, name);
    list.append(item);
  });

  section.append(bar, list);
  return section;
}
