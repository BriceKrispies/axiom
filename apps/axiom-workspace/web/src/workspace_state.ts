// Typed state for the workspace browser shell.
//
// This module owns the ENTIRE browser-side state shape. There is one typed
// state object per panel plus a root `WorkspaceBrowserState`. Every field is
// `readonly` and there is no `any` anywhere — the shell is a strictly typed
// view model.
//
// No runtime is attached yet, so `initialWorkspaceState()` starts EMPTY: every
// list is empty and every field is unset (there is no placeholder data). Panels
// render empty states until a real runtime, project, or asset source populates
// them. The shell never simulates real game behavior; its only state transition
// today is the workflow-mode switcher, through the single reducer in
// `workspace_events.ts`.

export type WorkspaceMode = "Edit" | "Play" | "DropIn" | "Replay" | "Package";

export interface ProjectRow {
  readonly id: string;
  readonly name: string;
}

export interface ProjectBrowserPanelState {
  readonly projects: readonly ProjectRow[];
  readonly selectedIndex: number | null;
}

export interface GameManifestEditorPanelState {
  readonly title: string;
  readonly version: string;
  readonly entrypoint: string;
  readonly defaultLevel: string;
}

export interface LevelRow {
  readonly id: string;
  readonly name: string;
}

export interface LevelBrowserPanelState {
  readonly levels: readonly LevelRow[];
  readonly selectedIndex: number | null;
}

// Which placeholder view the runtime viewport shows: the plain viewport
// placeholder, or the backend-comparison placeholder that mirrors the gallery's
// three-backend comparison (WebGPU / WebGL2 / Canvas2D).
export type RuntimeViewportView = "placeholder" | "triptych";

export interface ComparisonBackend {
  readonly id: string;
  readonly name: string;
  readonly note: string;
}

export interface RuntimeViewportPanelState {
  readonly placeholderLabel: string;
  readonly attached: boolean;
  readonly view: RuntimeViewportView;
  readonly backends: readonly ComparisonBackend[];
}

export interface ObjectInspectorField {
  readonly name: string;
  readonly value: string;
}

export interface ObjectInspectorPanelState {
  readonly selectedObjectId: string | null;
  readonly fields: readonly ObjectInspectorField[];
}

export interface AssetRow {
  readonly id: string;
  readonly kind: string;
  readonly name: string;
}

export interface AssetBrowserPanelState {
  readonly assets: readonly AssetRow[];
  readonly selectedIndex: number | null;
}

export interface ConsoleLogRecord {
  readonly level: string;
  readonly message: string;
  readonly tick: number;
}

export interface ConsoleLogViewerPanelState {
  readonly records: readonly ConsoleLogRecord[];
}

export interface ProfilerSample {
  readonly label: string;
  readonly micros: number;
  readonly tick: number;
}

export interface ProfilerPanelState {
  readonly samples: readonly ProfilerSample[];
}

export interface InputEventRow {
  readonly tick: number;
  readonly code: number;
  readonly label: string;
}

export interface InputDebuggerPanelState {
  readonly inputs: readonly InputEventRow[];
}

export interface TimelineTick {
  readonly tick: number;
  readonly snapshot: string | null;
}

export interface TimelineReplayPanelState {
  readonly ticks: readonly TimelineTick[];
  readonly replayRequested: boolean;
}

export interface PlayControlsPanelState {
  readonly mode: WorkspaceMode;
  readonly launchCreated: boolean;
  readonly dropInCreated: boolean;
}

export interface PackageExportPanelState {
  readonly status: string;
  readonly target: string;
  readonly requested: boolean;
}

export interface WorkspaceBrowserState {
  readonly mode: WorkspaceMode;
  readonly projectBrowser: ProjectBrowserPanelState;
  readonly gameManifestEditor: GameManifestEditorPanelState;
  readonly levelBrowser: LevelBrowserPanelState;
  readonly runtimeViewport: RuntimeViewportPanelState;
  readonly objectInspector: ObjectInspectorPanelState;
  readonly assetBrowser: AssetBrowserPanelState;
  readonly consoleLogViewer: ConsoleLogViewerPanelState;
  readonly profiler: ProfilerPanelState;
  readonly inputDebugger: InputDebuggerPanelState;
  readonly timelineReplay: TimelineReplayPanelState;
  readonly playControls: PlayControlsPanelState;
  readonly packageExport: PackageExportPanelState;
}

// Mirrors the Rust `GameManifestEditorState` sentinel: a field with no value.
export const UNSET = "<unset>";

// The shell attaches no real runtime yet, so it starts with NO data: every list
// is empty and every field is unset. Panels render empty states until a real
// runtime, project, or asset source is wired in — there is no placeholder data.
export function initialWorkspaceState(): WorkspaceBrowserState {
  return {
    mode: "Edit",
    projectBrowser: {
      projects: [],
      selectedIndex: null,
    },
    gameManifestEditor: {
      title: UNSET,
      version: UNSET,
      entrypoint: UNSET,
      defaultLevel: UNSET,
    },
    levelBrowser: {
      levels: [],
      selectedIndex: null,
    },
    runtimeViewport: {
      placeholderLabel: "No runtime attached",
      attached: false,
      view: "placeholder",
      backends: [],
    },
    objectInspector: {
      selectedObjectId: null,
      fields: [],
    },
    assetBrowser: {
      assets: [],
      selectedIndex: null,
    },
    consoleLogViewer: {
      records: [],
    },
    profiler: {
      samples: [],
    },
    inputDebugger: {
      inputs: [],
    },
    timelineReplay: {
      ticks: [],
      replayRequested: false,
    },
    playControls: {
      mode: "Edit",
      launchCreated: false,
      dropInCreated: false,
    },
    packageExport: {
      status: UNSET,
      target: UNSET,
      requested: false,
    },
  };
}
