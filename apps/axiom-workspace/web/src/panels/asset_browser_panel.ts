// Asset Browser panel (id: "asset-browser", region: right).
//
// Lists placeholder assets (id / kind / name). Selecting a row dispatches a typed
// `asset.placeholder.select` event through the shared reducer.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";

export function renderAssetBrowserPanel(
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "asset-browser";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Asset Browser";

  const note = document.createElement("p");
  note.className = "ws-panel-note";
  note.textContent = "Placeholder assets — click a row to select (shell state only).";

  const table = document.createElement("div");
  table.className = "ws-asset-table";

  state.assetBrowser.assets.forEach((asset, index) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ws-asset-row";
    const selected = state.assetBrowser.selectedIndex === index;
    button.setAttribute("aria-pressed", selected ? "true" : "false");
    button.dataset.selected = selected ? "true" : "false";

    const kind = document.createElement("span");
    kind.className = "ws-asset-kind";
    kind.textContent = asset.kind;
    const name = document.createElement("span");
    name.className = "ws-asset-name";
    name.textContent = asset.name;
    const id = document.createElement("span");
    id.className = "ws-asset-id";
    id.textContent = asset.id;

    button.append(kind, name, id);
    button.addEventListener("click", () => {
      dispatch({ type: "asset.placeholder.select", index });
    });
    table.append(button);
  });

  section.append(bar, note, table);
  return section;
}
