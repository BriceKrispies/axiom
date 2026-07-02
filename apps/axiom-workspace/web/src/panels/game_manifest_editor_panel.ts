// Game Manifest Editor panel (id: "game-manifest-editor", region: left).
//
// Shows the game manifest as TYPED, labeled placeholder fields (title / version
// / entrypoint / default level) — deliberately NOT a raw textarea blob. Read-only
// in this shell; editing is a future integration point.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";

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

  const note = document.createElement("p");
  note.className = "ws-panel-note";
  note.textContent = "Typed placeholder manifest fields (read-only in this shell).";

  const manifest = state.gameManifestEditor;
  const rows: readonly (readonly [string, string])[] = [
    ["title", manifest.title],
    ["version", manifest.version],
    ["entrypoint", manifest.entrypoint],
    ["defaultLevel", manifest.defaultLevel],
  ];

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

  section.append(bar, note, dl);
  return section;
}
