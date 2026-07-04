// Asset Browser panel (id: "asset-browser", region: right).
//
// A read-only, data-driven table of the opened project's assets (kind / name /
// id). Empty (no placeholder assets) until a real asset source is wired in.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";
import { renderEmpty } from "./empty_state";

export function renderAssetBrowserPanel(
  state: WorkspaceBrowserState,
  _dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "asset-browser";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Asset Browser";

  const assets = state.assetBrowser.assets;
  if (assets.length === 0) {
    section.append(bar, renderEmpty("No assets"));
    return section;
  }

  const table = document.createElement("div");
  table.className = "ws-asset-table";
  assets.forEach((asset, index) => {
    const row = document.createElement("div");
    row.className = "ws-asset-row";
    row.dataset.selected = state.assetBrowser.selectedIndex === index ? "true" : "false";

    const kind = document.createElement("span");
    kind.className = "ws-asset-kind";
    kind.textContent = asset.kind;
    const name = document.createElement("span");
    name.className = "ws-asset-name";
    name.textContent = asset.name;
    const id = document.createElement("span");
    id.className = "ws-asset-id";
    id.textContent = asset.id;

    row.append(kind, name, id);
    table.append(row);
  });

  section.append(bar, table);
  return section;
}
