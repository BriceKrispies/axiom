// Console Log Viewer panel (id: "console-log-viewer", region: bottom).
//
// A read-only, data-driven view over the console-log slice. No runtime is
// attached, so it stays empty until real log records arrive.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";
import { renderEmpty } from "./empty_state";

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

  const records = state.consoleLogViewer.records;
  if (records.length === 0) {
    section.append(bar, renderEmpty("No log records"));
    return section;
  }

  const list = document.createElement("ul");
  list.className = "ws-log-list";
  records.forEach((record) => {
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

  section.append(bar, list);
  return section;
}
