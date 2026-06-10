import { type Schema } from "./schema";
import type { Settings } from "./settings-mirror";
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

// Browse UI state. The editor draft is user-owned after selection/opening;
// incoming settings rebuild row values and flashes but must not overwrite it.
export type BrowseState = {
  schema: Schema | undefined;
  settings: Settings;
  root: string;
  editor: string;
  flashed: Set<string>;
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

type BrowseSettings = {
  settings: Settings;
  changed: Set<string>;
  rev?: string;
};

export function emptyState(): BrowseState {
  return {
    schema: undefined,
    settings: new Map(),
    root: "",
    editor: "null",
    flashed: new Set(),
    expanded: new Set(),
    selectedPath: "",
    userClosed: new Set(),
    tree: emptyTree(),
  };
}

export function selected(state: BrowseState): ViewNode | undefined {
  return state.tree.nodeByPath.get(state.selectedPath);
}

export function loadSchema(state: BrowseState, schema: Schema, subtreePath: string): BrowseState {
  const root = schema.path(subtreePath);
  return rebuild({
    ...state,
    schema,
    settings: new Map(),
    root,
    expanded: new Set(),
    selectedPath: "",
    userClosed: new Set(),
  });
}

export function commitSettings(state: BrowseState, { settings, changed, rev }: BrowseSettings): BrowseCommit {
  const rebuilt = rebuild({ ...state, settings }, false);
  return {
    state: {
      ...rebuilt,
      expanded: revealPresentSettings(
        rebuilt.expanded,
        rebuilt.userClosed,
        changed,
        rebuilt.settings,
        rebuilt.root,
      ),
    },
    cues: cuePaths(changed, rebuilt.root),
    rev: changed.size ? rev : undefined,
  };
}

export function setExpanded(state: BrowseState, path: string, open: boolean): BrowseState {
  const { expanded, userClosed } = toggleExpansion(state.expanded, state.userClosed, path, open);
  const selectedPath = !open && state.selectedPath !== path && state.selectedPath.startsWith(path ? `${path}/` : "/")
    ? path
    : state.selectedPath;
  return { ...state, expanded, userClosed, selectedPath };
}

export function select(state: BrowseState, path: string): BrowseState {
  return { ...state, selectedPath: path };
}

export function loadSelected(state: BrowseState, path: string): BrowseState {
  return loadEditor(select(state, path));
}

export function navigate(
  state: BrowseState,
  path: string,
  direction: NavDirection,
  step?: number,
): { state: BrowseState; path: string } {
  const next = movePath(visiblePaths(state), path, direction, state.tree.flatNodes, step);
  return { state: loadSelected(state, next), path: next };
}

export function updateEditor(state: BrowseState, editor: string): BrowseState {
  return { ...state, editor };
}

export function loadEditor(state: BrowseState): BrowseState {
  const node = selected(state);
  return {
    ...state,
    editor: node?.kind === "leaf" && node.present ? JSON.stringify(node.value, null, 2) : "null",
  };
}

export function setFlashed(state: BrowseState, flashed: Set<string>): BrowseState {
  return { ...state, flashed };
}

export function parseEditor(state: BrowseState): unknown {
  return JSON.parse(state.editor);
}

function visiblePaths(state: BrowseState): string[] {
  return visibleTreePaths(state.root, state.tree.flatNodes, state.expanded);
}

function rebuild(state: BrowseState, reloadEditor = true): BrowseState {
  const tree = treeSnapshot(state.schema, state.root, state.settings);
  const selectedPath = tree.nodes.some((node) => node.path === state.selectedPath)
    ? state.selectedPath
    : tree.nodes[0]?.path ?? "";
  const next = { ...state, tree, selectedPath };
  return reloadEditor ? loadEditor(next) : next;
}

function emptyTree(): TreeSnapshot {
  return {
    nodes: [],
    flatNodes: new Map(),
    nodeViews: new Map(),
    nodeByPath: new Map(),
  };
}
