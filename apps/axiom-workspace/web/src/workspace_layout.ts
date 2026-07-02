// Stable panel layout: the ordered list of panels and the region each lives in.
//
// This is the single source of truth for WHICH panels exist, in WHAT order, and
// in WHICH region of the shell. `dom_mount.ts` iterates this list to place each
// panel; `panel_registry.ts` maps each id to a render function.

export type WorkspaceRegion = "left" | "center" | "right" | "bottom";

export interface WorkspaceLayoutEntry {
  readonly id: string;
  readonly title: string;
  readonly region: WorkspaceRegion;
}

export const WORKSPACE_LAYOUT: readonly WorkspaceLayoutEntry[] = [
  { id: "project-browser", title: "Project Browser", region: "left" },
  { id: "game-manifest-editor", title: "Game Manifest Editor", region: "left" },
  { id: "level-browser", title: "Level Browser", region: "left" },
  { id: "runtime-viewport", title: "Runtime Viewport", region: "center" },
  { id: "object-inspector", title: "Object Inspector", region: "right" },
  { id: "asset-browser", title: "Asset Browser", region: "right" },
  { id: "package-export", title: "Package / Export", region: "right" },
  { id: "play-controls", title: "Play Controls", region: "bottom" },
  { id: "timeline-replay", title: "Timeline / Replay", region: "bottom" },
  { id: "console-log-viewer", title: "Console Log Viewer", region: "bottom" },
  { id: "profiler", title: "Profiler", region: "bottom" },
  { id: "input-debugger", title: "Input Debugger", region: "bottom" },
];

export const WORKSPACE_PANEL_IDS: readonly string[] = WORKSPACE_LAYOUT.map(
  (entry) => entry.id,
);
