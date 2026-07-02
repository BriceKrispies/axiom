// Explicit, typed workspace events + the single reducer.
//
// Every state change in the shell flows through `applyWorkspaceEvent`. There is
// no ad hoc mutation anywhere else. Events are a discriminated union keyed on a
// `type` string tag; the reducer switches exhaustively over that tag and returns
// a NEW `WorkspaceBrowserState` (clone + replace the relevant slice) for each.
//
// This is a SHELL reducer. It updates placeholder view state only — it must not
// simulate real game behavior. No `any` anywhere.

import type {
  ConsoleLogRecord,
  InputEventRow,
  RuntimeViewportView,
  TimelineTick,
  WorkspaceBrowserState,
  WorkspaceMode,
} from "./workspace_state";

export interface WorkspaceModeSetEvent {
  readonly type: "workspace.mode.set";
  readonly mode: WorkspaceMode;
}

export interface ProjectPlaceholderOpenEvent {
  readonly type: "project.placeholder.open";
  readonly projectId: string;
}

export interface LaunchPlaceholderCreateEvent {
  readonly type: "launch.placeholder.create";
}

export interface DropInPlaceholderCreateEvent {
  readonly type: "drop_in.placeholder.create";
  readonly levelId: string;
}

export interface InputPlaceholderRecordEvent {
  readonly type: "input.placeholder.record";
  readonly code: number;
  readonly label: string;
}

export interface SnapshotPlaceholderRecordEvent {
  readonly type: "snapshot.placeholder.record";
  readonly tick: number;
}

export interface ReplayPlaceholderCreateEvent {
  readonly type: "replay.placeholder.create";
}

export interface PackagePlaceholderRequestEvent {
  readonly type: "package.placeholder.request";
  readonly target: string;
}

export interface ObjectPlaceholderSelectEvent {
  readonly type: "object.placeholder.select";
  readonly objectId: string;
}

export interface AssetPlaceholderSelectEvent {
  readonly type: "asset.placeholder.select";
  readonly index: number;
}

export interface LevelPlaceholderSelectEvent {
  readonly type: "level.placeholder.select";
  readonly index: number;
}

export interface ViewportTabSetEvent {
  readonly type: "viewport.tab.set";
  readonly view: RuntimeViewportView;
}

export type WorkspaceEvent =
  | WorkspaceModeSetEvent
  | ProjectPlaceholderOpenEvent
  | LaunchPlaceholderCreateEvent
  | DropInPlaceholderCreateEvent
  | InputPlaceholderRecordEvent
  | SnapshotPlaceholderRecordEvent
  | ReplayPlaceholderCreateEvent
  | PackagePlaceholderRequestEvent
  | ObjectPlaceholderSelectEvent
  | AssetPlaceholderSelectEvent
  | LevelPlaceholderSelectEvent
  | ViewportTabSetEvent;

// The one dispatch shape threaded through the shell. No globals.
export type Dispatch = (event: WorkspaceEvent) => void;

function setMode(
  state: WorkspaceBrowserState,
  event: WorkspaceModeSetEvent,
): WorkspaceBrowserState {
  return {
    ...state,
    mode: event.mode,
    playControls: { ...state.playControls, mode: event.mode },
  };
}

function openProject(
  state: WorkspaceBrowserState,
  event: ProjectPlaceholderOpenEvent,
): WorkspaceBrowserState {
  const index = state.projectBrowser.projects.findIndex(
    (row) => row.id === event.projectId,
  );
  const selectedIndex = index >= 0 ? index : null;
  const record: ConsoleLogRecord = {
    level: "info",
    message: `opened placeholder project ${event.projectId}`,
    tick: 0,
  };
  return {
    ...state,
    projectBrowser: { ...state.projectBrowser, selectedIndex },
    consoleLogViewer: {
      records: [...state.consoleLogViewer.records, record],
    },
  };
}

function createLaunch(state: WorkspaceBrowserState): WorkspaceBrowserState {
  const record: ConsoleLogRecord = {
    level: "info",
    message: "created placeholder launch (shell state only)",
    tick: 0,
  };
  return {
    ...state,
    playControls: { ...state.playControls, launchCreated: true },
    consoleLogViewer: {
      records: [...state.consoleLogViewer.records, record],
    },
  };
}

function createDropIn(
  state: WorkspaceBrowserState,
  event: DropInPlaceholderCreateEvent,
): WorkspaceBrowserState {
  const record: ConsoleLogRecord = {
    level: "info",
    message: `created placeholder drop-in for ${event.levelId}`,
    tick: 0,
  };
  return {
    ...state,
    playControls: { ...state.playControls, dropInCreated: true },
    consoleLogViewer: {
      records: [...state.consoleLogViewer.records, record],
    },
  };
}

function recordInput(
  state: WorkspaceBrowserState,
  event: InputPlaceholderRecordEvent,
): WorkspaceBrowserState {
  const nextTick = state.inputDebugger.inputs.length * 10;
  const row: InputEventRow = {
    tick: nextTick,
    code: event.code,
    label: event.label,
  };
  return {
    ...state,
    inputDebugger: { inputs: [...state.inputDebugger.inputs, row] },
  };
}

function recordSnapshot(
  state: WorkspaceBrowserState,
  event: SnapshotPlaceholderRecordEvent,
): WorkspaceBrowserState {
  const tick: TimelineTick = {
    tick: event.tick,
    snapshot: `placeholder-snapshot-${event.tick.toString().padStart(4, "0")}`,
  };
  return {
    ...state,
    timelineReplay: {
      ...state.timelineReplay,
      ticks: [...state.timelineReplay.ticks, tick],
    },
  };
}

function createReplay(state: WorkspaceBrowserState): WorkspaceBrowserState {
  const record: ConsoleLogRecord = {
    level: "info",
    message: "requested placeholder replay (shell state only)",
    tick: 0,
  };
  return {
    ...state,
    timelineReplay: { ...state.timelineReplay, replayRequested: true },
    consoleLogViewer: {
      records: [...state.consoleLogViewer.records, record],
    },
  };
}

function requestPackage(
  state: WorkspaceBrowserState,
  event: PackagePlaceholderRequestEvent,
): WorkspaceBrowserState {
  return {
    ...state,
    packageExport: {
      status: "requested (placeholder)",
      target: event.target,
      requested: true,
    },
  };
}

function selectObject(
  state: WorkspaceBrowserState,
  event: ObjectPlaceholderSelectEvent,
): WorkspaceBrowserState {
  return {
    ...state,
    objectInspector: {
      ...state.objectInspector,
      selectedObjectId: event.objectId,
    },
  };
}

function selectAsset(
  state: WorkspaceBrowserState,
  event: AssetPlaceholderSelectEvent,
): WorkspaceBrowserState {
  return {
    ...state,
    assetBrowser: { ...state.assetBrowser, selectedIndex: event.index },
  };
}

function selectLevel(
  state: WorkspaceBrowserState,
  event: LevelPlaceholderSelectEvent,
): WorkspaceBrowserState {
  return {
    ...state,
    levelBrowser: { ...state.levelBrowser, selectedIndex: event.index },
  };
}

function setViewportTab(
  state: WorkspaceBrowserState,
  event: ViewportTabSetEvent,
): WorkspaceBrowserState {
  return {
    ...state,
    runtimeViewport: { ...state.runtimeViewport, view: event.view },
  };
}

export function applyWorkspaceEvent(
  state: WorkspaceBrowserState,
  event: WorkspaceEvent,
): WorkspaceBrowserState {
  switch (event.type) {
    case "workspace.mode.set":
      return setMode(state, event);
    case "project.placeholder.open":
      return openProject(state, event);
    case "launch.placeholder.create":
      return createLaunch(state);
    case "drop_in.placeholder.create":
      return createDropIn(state, event);
    case "input.placeholder.record":
      return recordInput(state, event);
    case "snapshot.placeholder.record":
      return recordSnapshot(state, event);
    case "replay.placeholder.create":
      return createReplay(state);
    case "package.placeholder.request":
      return requestPackage(state, event);
    case "object.placeholder.select":
      return selectObject(state, event);
    case "asset.placeholder.select":
      return selectAsset(state, event);
    case "level.placeholder.select":
      return selectLevel(state, event);
    case "viewport.tab.set":
      return setViewportTab(state, event);
    default:
      return assertNever(event);
  }
}

function assertNever(event: never): never {
  throw new Error(`unhandled workspace event: ${JSON.stringify(event)}`);
}
