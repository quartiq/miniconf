import { type Schema } from "./schema";
import type { Settings, SettingsCommit } from "./settings-mirror";
import {
  cuePaths,
  revealPresentSettings,
  treeSnapshot,
  type TreeSnapshot,
  type ViewNode,
} from "./tree-state";
import {
  movePath,
  toggleExpansion,
  visibleTreePaths,
  type NavDirection,
} from "./tree-navigation";

// Undefined follows the device; any string (including empty) is a local edit.
// Selection, Revert and successful Set return to following the device.
export type BrowseState = {
  schema: Schema | undefined;
  settings: Settings;
  root: string;
  draft: string | undefined;
  expanded: Set<string>;
  selectedPath: string;
  userClosed: Set<string>;
  tree: TreeSnapshot;
};

export type BrowseCommit = {
  state: BrowseState;
  cues: Set<string>;
  rev?: string;
};

export type BrowseMemory = Pick<
  BrowseState,
  "expanded" | "selectedPath" | "userClosed"
>;

export function emptyState(): BrowseState {
  return {
    schema: undefined,
    settings: new Map(),
    root: "",
    draft: undefined,
    expanded: new Set(),
    selectedPath: "",
    userClosed: new Set(),
    tree: emptyTree(),
  };
}

export function selected(state: BrowseState): ViewNode | undefined {
  return state.tree.nodeByPath.get(state.selectedPath);
}

export function editor(state: BrowseState): string {
  return state.draft ?? selected(state)?.value ?? "";
}

export function loadSchema(
  state: BrowseState,
  schema: Schema,
  subtreePath: string,
  memory: BrowseMemory = state,
): BrowseState {
  const root = schema.path(subtreePath);
  const tree = treeSnapshot(schema, root, state.settings);
  const branches = new Set(
    [...tree.nodeViews.values()]
      .filter(({ children }) => children.length)
      .map(({ path }) => path),
  );
  return {
    ...state,
    schema,
    root,
    tree,
    selectedPath:
      state.draft !== undefined || tree.nodeByPath.has(memory.selectedPath)
        ? memory.selectedPath
        : root,
    expanded: new Set(
      [...memory.expanded].filter((path) => branches.has(path)),
    ),
    userClosed: new Set(
      [...memory.userClosed].filter((path) => branches.has(path)),
    ),
  };
}

export function commitSettings(
  state: BrowseState,
  { settings, touched, activity, rev }: SettingsCommit,
): BrowseCommit {
  const rebuilt = {
    ...state,
    settings,
    tree: treeSnapshot(state.schema, state.root, settings),
  };
  return {
    state: {
      ...rebuilt,
      expanded: revealPresentSettings(
        rebuilt.expanded,
        rebuilt.userClosed,
        touched,
        rebuilt.settings,
        rebuilt.root,
      ),
    },
    cues: cuePaths(activity, rebuilt.root),
    rev: touched.size ? rev : undefined,
  };
}

export function setExpanded(
  state: BrowseState,
  path: string,
  open: boolean,
): BrowseState {
  const { expanded, userClosed } = toggleExpansion(
    state.expanded,
    state.userClosed,
    path,
    open,
  );
  // Folding changes visibility, not the selected leaf or its draft. The tree
  // supplies a visible keyboard entry point independently of selection.
  return { ...state, expanded, userClosed };
}

export function loadSelected(state: BrowseState, path: string): BrowseState {
  if (path === state.selectedPath) return state;
  return loadEditor({ ...state, selectedPath: path });
}

export function navigate(
  state: BrowseState,
  path: string,
  direction: NavDirection,
  step?: number,
): { state: BrowseState; path: string } {
  const next = movePath(
    visiblePaths(state),
    path,
    direction,
    state.tree.nodeViews,
    step,
  );
  return { state: loadSelected(state, next), path: next };
}

export function updateEditor(state: BrowseState, editor: string): BrowseState {
  return {
    ...state,
    draft: editor === selected(state)?.value ? undefined : editor,
  };
}

export function loadEditor(state: BrowseState): BrowseState {
  return { ...state, draft: undefined };
}

function visiblePaths(state: BrowseState): string[] {
  return visibleTreePaths(state.root, state.tree.nodeViews, state.expanded);
}

function emptyTree(): TreeSnapshot {
  return {
    nodeViews: new Map(),
    nodeByPath: new Map(),
  };
}
