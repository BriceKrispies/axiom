// Runtime Viewport panel (id: "runtime-viewport", region: center).
//
// The large central region. It has two toggleable placeholder views, switched by
// a tab bar and driven through the single reducer (event "viewport.tab.set"):
//
//   - "Viewport"          — the plain "Runtime Viewport Placeholder" surface.
//   - "Backend Triptych"  — a placeholder that names the same three render
//                           backends the demo gallery compares side by side
//                           (WebGPU / WebGL2 / Canvas2D).
//
// No runtime is attached and no live surface is embedded here. The LIVE backend
// comparison — one deterministic demo (e.g. the retro_fps demo) rendered through all
// three backends at once — runs in the demo gallery (apps/axiom-gallery), which
// owns the real render surfaces. This panel only mirrors that comparison as
// labeled placeholder data; wiring a real surface in is a future integration point.

import type { Dispatch } from "../workspace_events";
import type {
  RuntimeViewportView,
  WorkspaceBrowserState,
} from "../workspace_state";

interface ViewportTab {
  readonly view: RuntimeViewportView;
  readonly label: string;
}

const VIEWPORT_TABS: readonly ViewportTab[] = [
  { view: "placeholder", label: "Viewport" },
  { view: "triptych", label: "Backend Triptych" },
];

function renderTabBar(
  active: RuntimeViewportView,
  dispatch: Dispatch,
): HTMLElement {
  const tabs = document.createElement("div");
  tabs.className = "ws-viewport-tabs";
  for (const tab of VIEWPORT_TABS) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ws-viewport-tab";
    button.textContent = tab.label;
    button.dataset.active = String(tab.view === active);
    button.addEventListener("click", () => {
      dispatch({ type: "viewport.tab.set", view: tab.view });
    });
    tabs.append(button);
  }
  return tabs;
}

function renderPlaceholderView(state: WorkspaceBrowserState): HTMLElement {
  const stage = document.createElement("div");
  stage.className = "ws-viewport-stage";

  const label = document.createElement("span");
  label.className = "ws-viewport-label";
  label.textContent = state.runtimeViewport.placeholderLabel;

  const mode = document.createElement("span");
  mode.className = "ws-viewport-mode";
  mode.textContent = `mode: ${state.mode} · attached: ${String(state.runtimeViewport.attached)}`;

  stage.append(label, mode);
  return stage;
}

function renderTriptychView(state: WorkspaceBrowserState): HTMLElement {
  const stage = document.createElement("div");
  stage.className = "ws-viewport-stage ws-triptych-stage";

  const heading = document.createElement("div");
  heading.className = "ws-triptych-heading";
  heading.textContent = "Backend Triptych Placeholder";

  const panes = document.createElement("div");
  panes.className = "ws-triptych-panes";
  for (const backend of state.runtimeViewport.backends) {
    const pane = document.createElement("div");
    pane.className = "ws-triptych-pane";

    const paneLabel = document.createElement("div");
    paneLabel.className = "ws-triptych-pane-label";
    const name = document.createElement("span");
    name.className = "ws-triptych-pane-name";
    name.textContent = backend.name;
    const note = document.createElement("span");
    note.className = "ws-triptych-pane-note";
    note.textContent = backend.note;
    paneLabel.append(name, note);

    const body = document.createElement("div");
    body.className = "ws-triptych-pane-body";
    body.textContent = `${backend.name} placeholder`;

    pane.append(paneLabel, body);
    panes.append(pane);
  }

  const hint = document.createElement("p");
  hint.className = "ws-triptych-hint";
  hint.textContent =
    "Placeholder — the live comparison renders one deterministic demo (e.g. the " +
    "retro_fps demo) through all three backends at once in the demo gallery " +
    "(apps/axiom-gallery). This shell embeds no live surface.";

  stage.append(heading, panes, hint);
  return stage;
}

export function renderRuntimeViewportPanel(
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
): HTMLElement {
  const section = document.createElement("section");
  section.className = "ws-panel ws-viewport-panel";
  section.dataset.panelId = "runtime-viewport";

  const bar = document.createElement("div");
  bar.className = "ws-panel-title";
  bar.textContent = "Runtime Viewport";

  const active = state.runtimeViewport.view;
  const content =
    active === "triptych"
      ? renderTriptychView(state)
      : renderPlaceholderView(state);

  section.append(bar, renderTabBar(active, dispatch), content);
  return section;
}
