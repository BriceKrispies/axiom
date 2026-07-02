// Timeline / Replay panel (id: "timeline-replay", region: bottom).
//
// Shows placeholder ticks and their snapshot markers (tick order), plus a
// placeholder "record snapshot" button and a placeholder "replay" button. Both
// dispatch typed events; neither drives a real runtime.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";

export function renderTimelineReplayPanel(
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "timeline-replay";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Timeline / Replay";

  const controls = document.createElement("div");
  controls.className = "ws-button-row";

  const snapshot = document.createElement("button");
  snapshot.type = "button";
  snapshot.className = "ws-inline-button";
  snapshot.textContent = "Record Placeholder Snapshot";
  snapshot.addEventListener("click", () => {
    const nextTick = state.timelineReplay.ticks.length * 60;
    dispatch({ type: "snapshot.placeholder.record", tick: nextTick });
  });

  const replay = document.createElement("button");
  replay.type = "button";
  replay.className = "ws-inline-button";
  replay.textContent = "Replay (placeholder)";
  replay.addEventListener("click", () => {
    dispatch({ type: "replay.placeholder.create" });
  });

  controls.append(snapshot, replay);

  const status = document.createElement("p");
  status.className = "ws-panel-note";
  status.textContent = `replayRequested: ${String(state.timelineReplay.replayRequested)}`;

  const list = document.createElement("ul");
  list.className = "ws-timeline-list";

  state.timelineReplay.ticks.forEach((entry) => {
    const item = document.createElement("li");
    item.className = "ws-timeline-row";

    const tick = document.createElement("span");
    tick.className = "ws-timeline-tick";
    tick.textContent = `t${entry.tick}`;
    const snap = document.createElement("span");
    snap.className = "ws-timeline-snapshot";
    snap.textContent = entry.snapshot ?? "no snapshot (placeholder)";

    item.append(tick, snap);
    list.append(item);
  });

  section.append(bar, controls, status, list);
  return section;
}
