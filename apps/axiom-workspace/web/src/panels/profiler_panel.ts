// Profiler panel (id: "profiler", region: bottom).
//
// A read-only, data-driven view over the profiler slice (label / micros / tick).
// No runtime is attached, so it stays empty until real timing samples arrive.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";
import { renderEmpty } from "./empty_state";

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

  const samples = state.profiler.samples;
  if (samples.length === 0) {
    section.append(bar, renderEmpty("No timing samples"));
    return section;
  }

  const list = document.createElement("ul");
  list.className = "ws-sample-list";
  samples.forEach((sample) => {
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

  section.append(bar, list);
  return section;
}
