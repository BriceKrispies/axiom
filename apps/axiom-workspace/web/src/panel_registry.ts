// Panel registry: maps each stable panel id to its single render function.
//
// Every panel module exports exactly one render function with the SAME signature
// `(state, dispatch) => HTMLElement`. This registry is the one place that knows
// the id -> renderer mapping; `dom_mount.ts` looks panels up here in layout order.

import type { Dispatch } from "./workspace_events";
import type { WorkspaceBrowserState } from "./workspace_state";

import { renderAssetBrowserPanel } from "./panels/asset_browser_panel";
import { renderConsoleLogViewerPanel } from "./panels/console_log_viewer_panel";
import { renderGameManifestEditorPanel } from "./panels/game_manifest_editor_panel";
import { renderInputDebuggerPanel } from "./panels/input_debugger_panel";
import { renderLevelBrowserPanel } from "./panels/level_browser_panel";
import { renderObjectInspectorPanel } from "./panels/object_inspector_panel";
import { renderPackageExportPanel } from "./panels/package_export_panel";
import { renderPlayControlsPanel } from "./panels/play_controls_panel";
import { renderProfilerPanel } from "./panels/profiler_panel";
import { renderProjectBrowserPanel } from "./panels/project_browser_panel";
import { renderRuntimeViewportPanel } from "./panels/runtime_viewport_panel";
import { renderTimelineReplayPanel } from "./panels/timeline_replay_panel";

export type PanelRenderer = (
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
) => HTMLElement;

export const PANEL_RENDERERS: Readonly<Record<string, PanelRenderer>> = {
  "project-browser": renderProjectBrowserPanel,
  "game-manifest-editor": renderGameManifestEditorPanel,
  "level-browser": renderLevelBrowserPanel,
  "runtime-viewport": renderRuntimeViewportPanel,
  "object-inspector": renderObjectInspectorPanel,
  "asset-browser": renderAssetBrowserPanel,
  "package-export": renderPackageExportPanel,
  "play-controls": renderPlayControlsPanel,
  "timeline-replay": renderTimelineReplayPanel,
  "console-log-viewer": renderConsoleLogViewerPanel,
  "profiler": renderProfilerPanel,
  "input-debugger": renderInputDebuggerPanel,
};

export function renderPanel(
  id: string,
  state: WorkspaceBrowserState,
  dispatch: Dispatch,
): HTMLElement {
  const renderer = PANEL_RENDERERS[id];
  if (renderer === undefined) {
    const fallback = document.createElement("section");
    fallback.className = "ws-panel";
    const bar = document.createElement("div");
    bar.className = "ws-panel-title";
    bar.textContent = `Unknown Panel: ${id}`;
    fallback.append(bar);
    return fallback;
  }
  return renderer(state, dispatch);
}
