// Console Log Viewer panel (id: "console-log-viewer", region: bottom).
//
// Shows structured placeholder log records (level / message / tick) in insertion
// order. This is a read-only view over the console-log slice of shell state.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";

export function renderConsoleLogViewerPanel(
  state: WorkspaceBrowserState,
  _dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "console-log-viewer";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Console Log Viewer";

  const note = document.createElement("p");
  note.className = "ws-panel-note";
  note.textContent = "Placeholder log records (insertion order).";

  const list = document.createElement("ul");
  list.className = "ws-log-list";

  state.consoleLogViewer.records.forEach((record) => {
    const item = document.createElement("li");
    item.className = "ws-log-row";

    const level = document.createElement("span");
    level.className = "ws-log-level";
    level.dataset.level = record.level;
    level.textContent = record.level;
    const tick = document.createElement("span");
    tick.className = "ws-log-tick";
    tick.textContent = `t${record.tick}`;
    const message = document.createElement("span");
    message.className = "ws-log-message";
    message.textContent = record.message;

    item.append(level, tick, message);
    list.append(item);
  });

  section.append(bar, note, list);
  return section;
}
