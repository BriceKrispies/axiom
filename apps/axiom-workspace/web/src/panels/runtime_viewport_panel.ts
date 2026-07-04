// Runtime Viewport panel (id: "runtime-viewport", region: center).
//
// The large central region where a live runtime surface will mount. No runtime is
// attached and no live surface is embedded here (the LIVE backend comparison — one
// deterministic app rendered through all three backends at once — runs in the
// workspace's runtime console and the demo gallery, which own the real surfaces).
// Until a surface is wired in, this shows an empty "no runtime attached" stage.

import type { Dispatch } from "../workspace_events";
import type { WorkspaceBrowserState } from "../workspace_state";

export function renderRuntimeViewportPanel(
  state: WorkspaceBrowserState,
  _dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel ws-viewport-panel";
  section.dataset.panelId = "runtime-viewport";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Runtime Viewport";

  const stage = document.createElement("div");
  stage.className = "ws-viewport-stage";

  const label = document.createElement("span");
  label.className = "ws-viewport-label";
  label.textContent = state.runtimeViewport.placeholderLabel;

  const mode = document.createElement("span");
  mode.className = "ws-viewport-mode";
  mode.textContent = `mode: ${state.mode} · attached: ${String(state.runtimeViewport.attached)}`;

  stage.append(label, mode);
  section.append(bar, stage);
  return section;
}
