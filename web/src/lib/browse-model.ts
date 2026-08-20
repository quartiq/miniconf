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
  editorDirty: boolean;
  editorStale: boolean;
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

export type BrowseMemory = Pick<BrowseState, "expanded" | "selectedPath" | "userClosed">;

type BrowseSettings = {
  settings: Settings;
  changed: Set<string>;
  activity: Set<string>;
  rev?: string;
};

export function emptyState(): BrowseState {
  return {
    schema: undefined,
    settings: new Map(),
    root: "",
    editor: "null",
    editorDirty: false,
    editorStale: false,
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
  const next = rebuild({
    ...state,
    schema,
    settings: state.settings,
    root,
    expanded: new Set(),
    selectedPath: memory.selectedPath,
    userClosed: new Set(),
  });
  const branches = new Set(
    [...next.tree.flatNodes.values()].filter(({ children }) => children.length).map(({ path }) => path),
  );
  return {
    ...next,
    expanded: new Set([...memory.expanded].filter((path) => branches.has(path))),
    userClosed: new Set([...memory.userClosed].filter((path) => branches.has(path))),
  };
}

export function commitSettings(state: BrowseState, { settings, changed, activity, rev }: BrowseSettings): BrowseCommit {
  let rebuilt = rebuild({ ...state, settings }, false);
  if (changed.has(rebuilt.selectedPath)) {
    if (!state.editorDirty || draftMatches(state.editor, selected(rebuilt))) {
      rebuilt = loadEditor(rebuilt);
    } else {
      rebuilt = { ...rebuilt, editorStale: true };
    }
  }
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
    cues: cuePaths(activity, rebuilt.root),
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
  return { ...state, editor, editorDirty: true };
}

export function loadEditor(state: BrowseState): BrowseState {
  const node = selected(state);
  return {
    ...state,
    editor: node?.kind === "leaf" && node.present ? JSON.stringify(node.value, null, 2) : "null",
    editorDirty: false,
    editorStale: false,
  };
}

function draftMatches(editor: string, node: ViewNode | undefined): boolean {
  if (node?.kind !== "leaf" || !node.present) {
    return editor.trim() === "null";
  }
  try {
    return jsonEqual(JSON.parse(editor), node.value);
  } catch {
    return false;
  }
}

function jsonEqual(left: unknown, right: unknown): boolean {
  if (left === right) return true;
  if (Array.isArray(left) || Array.isArray(right)) {
    return Array.isArray(left) && Array.isArray(right) &&
      left.length === right.length && left.every((value, index) => jsonEqual(value, right[index]));
  }
  if (!left || !right || typeof left !== "object" || typeof right !== "object") return false;
  const leftObject = left as Record<string, unknown>;
  const rightObject = right as Record<string, unknown>;
  const keys = Object.keys(leftObject);
  return keys.length === Object.keys(rightObject).length &&
    keys.every((key) => Object.hasOwn(rightObject, key) && jsonEqual(leftObject[key], rightObject[key]));
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
