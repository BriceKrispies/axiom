// Timeline / Replay panel (id: "timeline-replay", region: bottom).
//
// A read-only, data-driven list of recorded ticks and their snapshot markers. No
// runtime is attached, so it stays empty until a real session is recorded.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";
import { renderEmpty } from "./empty_state";

export function renderTimelineReplayPanel(
  state: WorkspaceBrowserState,
  _dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "timeline-replay";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Timeline / Replay";

  const ticks = state.timelineReplay.ticks;
  if (ticks.length === 0) {
    section.append(bar, renderEmpty("No recorded session"));
    return section;
  }

  const list = document.createElement("ul");
  list.className = "ws-timeline-list";
  ticks.forEach((entry) => {
    const item = document.createElement("li");
    item.className = "ws-timeline-row";

    const tick = document.createElement("span");
    tick.className = "ws-timeline-tick";
    tick.textContent = `t${entry.tick}`;
    const snap = document.createElement("span");
    snap.className = "ws-timeline-snapshot";
    snap.textContent = entry.snapshot ?? "—";

    item.append(tick, snap);
    list.append(item);
  });

  section.append(bar, list);
  return section;
}
