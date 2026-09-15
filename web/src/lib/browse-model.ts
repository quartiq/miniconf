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

// The editor follows the device while equal to its last observed value.
// Differing text is a local edit; selection, Revert and successful Set replace it.
export type BrowseState = {
  schema: Schema | undefined;
  settings: Settings;
  root: string;
  editor: string;
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
    editor: "",
    expanded: new Set(),
    selectedPath: "",
    userClosed: new Set(),
    tree: emptyTree(),
  };
}

export function selected(state: BrowseState): ViewNode | undefined {
  return state.tree.nodeByPath.get(state.selectedPath);
}

export function loadSchema(
  state: BrowseState,
  schema: Schema,
  subtreePath: string,
  memory: BrowseMemory = state,
): BrowseState {
  const root = schema.path(subtreePath);
  let next = rebuild({
    ...state,
    schema,
    settings: state.settings,
    root,
    expanded: new Set(),
    selectedPath: memory.selectedPath,
    userClosed: new Set(),
  });
  if (
    state.editor === (selected(state)?.value ?? "") &&
    !next.tree.nodeByPath.has(next.selectedPath)
  ) {
    next = { ...next, selectedPath: next.tree.nodes[0]?.path ?? "" };
  }
  const branches = new Set(
    [...next.tree.nodeViews.values()]
      .filter(({ children }) => children.length)
      .map(({ path }) => path),
  );
  return {
    ...(state.editor !== (selected(state)?.value ?? "")
      ? next
      : loadEditor(next)),
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
  let rebuilt = rebuild({ ...state, settings });
  if (
    touched.has(state.selectedPath) &&
    state.editor === (selected(state)?.value ?? "")
  ) {
    rebuilt = loadEditor(rebuilt);
  }
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
  return { ...state, editor };
}

export function loadEditor(state: BrowseState): BrowseState {
  const node = selected(state);
  const text = node?.kind === "leaf" ? node.value : undefined;
  return { ...state, editor: text ?? "" };
}

function visiblePaths(state: BrowseState): string[] {
  return visibleTreePaths(state.root, state.tree.nodeViews, state.expanded);
}

function rebuild(state: BrowseState): BrowseState {
  return {
    ...state,
    tree: treeSnapshot(state.schema, state.root, state.settings),
  };
}

function emptyTree(): TreeSnapshot {
  return {
    nodes: [],
    nodeViews: new Map(),
    nodeByPath: new Map(),
  };
}
