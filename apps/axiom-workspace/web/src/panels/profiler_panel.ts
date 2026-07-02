// Profiler panel (id: "profiler", region: bottom).
//
// Shows placeholder frame/system timing samples (label / micros / tick) in
// insertion order. Read-only view over the profiler slice of shell state.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";

export function renderProfilerPanel(
  state: WorkspaceBrowserState,
  _dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel";
  section.dataset.panelId = "profiler";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Profiler";

  const note = document.createElement("p");
  note.className = "ws-panel-note";
  note.textContent = "Placeholder timing samples (insertion order).";

  const list = document.createElement("ul");
  list.className = "ws-sample-list";

  state.profiler.samples.forEach((sample) => {
    const item = document.createElement("li");
    item.className = "ws-sample-row";

    const label = document.createElement("span");
    label.className = "ws-sample-label";
    label.textContent = sample.label;
    const micros = document.createElement("span");
    micros.className = "ws-sample-micros";
    micros.textContent = `${sample.micros} us`;
    const tick = document.createElement("span");
    tick.className = "ws-sample-tick";
    tick.textContent = `t${sample.tick}`;

    item.append(label, micros, tick);
    list.append(item);
  });

  section.append(bar, note, list);
  return section;
}
