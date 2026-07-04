// Game Manifest Editor panel (id: "game-manifest-editor", region: left).
//
// A read-only view of the opened game's manifest fields (title / version /
// entrypoint / default level). No project is open, so every field is unset and it
// shows an empty state until a real manifest is loaded.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";
import { UNSET } from "../workspace_state";
import { renderEmpty } from "./empty_state";

export function renderGameManifestEditorPanel(
  state: WorkspaceBrowserState,
  _dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "game-manifest-editor";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Game Manifest Editor";

  const manifest = state.gameManifestEditor;
  const rows: readonly (readonly [string, string])[] = [
    ["title", manifest.title],
    ["version", manifest.version],
    ["entrypoint", manifest.entrypoint],
    ["defaultLevel", manifest.defaultLevel],
  ];
  if (rows.every(([, value]) => value === UNSET)) {
    section.append(bar, renderEmpty("No game manifest loaded"));
    return section;
  }

  const dl = document.createElement("dl");
  dl.className = "ws-field-list";
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
