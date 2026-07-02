// Typed placeholder state for the workspace browser shell.
//
// This module owns the ENTIRE browser-side state shape. There is one typed
// state object per panel plus a root `WorkspaceBrowserState`. Every field is
// `readonly` and there is no `any` anywhere — the shell is a strictly typed
// view model.
//
// This is a PLACEHOLDER shell: `initialWorkspaceState()` fills each panel with
// a few clearly-labeled placeholder rows so the panels render visibly populated.
// The shell never simulates real game behavior; it is a developer surface whose
// panels reflect typed state, and whose only state transitions go through the
// single reducer in `workspace_events.ts`.

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
  readonly placeholderLabel: "Runtime Viewport Placeholder";
  readonly attached: false;
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

export function initialWorkspaceState(): WorkspaceBrowserState {
  return {
    mode: "Edit",
    projectBrowser: {
      projects: [
        { id: "proj.placeholder.alpha", name: "Placeholder Project Alpha" },
        { id: "proj.placeholder.beta", name: "Placeholder Project Beta" },
        { id: "proj.placeholder.gamma", name: "Placeholder Project Gamma" },
      ],
      selectedIndex: null,
    },
    gameManifestEditor: {
      title: "Placeholder Game Title",
      version: "0.0.0-placeholder",
      entrypoint: "placeholder_main",
      defaultLevel: "level.placeholder.01",
    },
    levelBrowser: {
      levels: [
        { id: "level.placeholder.01", name: "Placeholder Level 01" },
        { id: "level.placeholder.02", name: "Placeholder Level 02" },
        { id: "level.placeholder.03", name: "Placeholder Level 03" },
      ],
      selectedIndex: null,
    },
    runtimeViewport: {
      placeholderLabel: "Runtime Viewport Placeholder",
      attached: false,
      view: "placeholder",
      backends: [
        { id: "webgpu", name: "WebGPU", note: "GPU · primary" },
        { id: "webgl2", name: "WebGL2", note: "GPU · fallback" },
        { id: "canvas2d", name: "Canvas2D", note: "software rasterizer" },
      ],
    },
    objectInspector: {
      selectedObjectId: null,
      fields: [
        { name: "placeholder.name", value: "placeholder value" },
        { name: "placeholder.transform", value: "0, 0, 0 (placeholder)" },
        { name: "placeholder.tag", value: "unset (placeholder)" },
      ],
    },
    assetBrowser: {
      assets: [
        { id: "asset.placeholder.mesh.01", kind: "mesh", name: "Placeholder Mesh" },
        { id: "asset.placeholder.texture.01", kind: "texture", name: "Placeholder Texture" },
        { id: "asset.placeholder.audio.01", kind: "audio", name: "Placeholder Audio Clip" },
      ],
      selectedIndex: null,
    },
    consoleLogViewer: {
      records: [
        { level: "info", message: "workspace shell opened (placeholder)", tick: 0 },
        { level: "info", message: "no runtime attached (placeholder)", tick: 0 },
        { level: "warn", message: "all data below is placeholder data", tick: 0 },
      ],
    },
    profiler: {
      samples: [
        { label: "placeholder.frame", micros: 16_666, tick: 0 },
        { label: "placeholder.update", micros: 4_200, tick: 0 },
        { label: "placeholder.render", micros: 8_100, tick: 0 },
      ],
    },
    inputDebugger: {
      inputs: [
        { tick: 0, code: 0x01, label: "placeholder.move.forward" },
        { tick: 0, code: 0x02, label: "placeholder.move.back" },
        { tick: 0, code: 0x20, label: "placeholder.jump" },
      ],
    },
    timelineReplay: {
      ticks: [
        { tick: 0, snapshot: "placeholder-snapshot-0000" },
        { tick: 60, snapshot: "placeholder-snapshot-0060" },
        { tick: 120, snapshot: null },
      ],
      replayRequested: false,
    },
    playControls: {
      mode: "Edit",
      launchCreated: false,
      dropInCreated: false,
    },
    packageExport: {
      status: "idle (placeholder)",
      target: "browser (placeholder)",
      requested: false,
    },
  };
}
