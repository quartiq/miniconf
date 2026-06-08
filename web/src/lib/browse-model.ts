import { displayPath, type Schema } from "./schema";
import type { Settings } from "./settings-mirror";
import {
  cuePaths,
  revealPresentSettings,
  treeSnapshot,
  type TreeSnapshot,
  type ViewNode,
} from "./tree-state";
import { movePath, toggleExpansion, visibleTreePaths, type NavDirection } from "./tree-navigation";

// Pure browse UI state. The editor draft is user-owned after selection/opening;
// incoming settings rebuild row values and flashes but must not overwrite it.
export type BrowseCommit = {
  cues: Set<string>;
  rev?: string;
  status: string;
};

type BrowseSettings = {
  settings: Settings;
  changed: Set<string>;
  rev?: string;
};

export class BrowseModel {
  private userClosed = new Set<string>();

  schema: Schema | undefined;
  settings: Settings = new Map();
  root = "";
  expanded = new Set<string>();
  selectedPath = "";
  editor = "null";
  flashed = new Set<string>();
  private tree = emptyTree();

  get selected(): ViewNode | undefined {
    return this.tree.nodeByPath.get(this.selectedPath);
  }

  get rootNode(): ViewNode | undefined {
    return this.tree.nodeByPath.get(this.root);
  }

  get treeNodes() {
    return this.tree.nodeViews;
  }

  get visiblePaths(): string[] {
    return visibleTreePaths(this.root, this.tree.flatNodes, this.expanded);
  }

  reset(): void {
    this.userClosed = new Set();
    this.schema = undefined;
    this.settings = new Map();
    this.root = "";
    this.tree = emptyTree();
    this.expanded = new Set();
    this.selectedPath = "";
    this.editor = "null";
    this.flashed = new Set();
  }

  loadSchema(schema: Schema, subtreePath: string): string {
    this.schema = schema;
    this.root = schema.path(subtreePath);
    this.settings = new Map();
    this.expanded = new Set();
    this.userClosed = new Set();
    this.rebuild();
    return this.root;
  }

  commit({ settings, changed, rev }: BrowseSettings): BrowseCommit {
    this.settings = settings;
    this.rebuild(false);
    this.expanded = revealPresentSettings(
      this.expanded,
      this.userClosed,
      changed,
      this.settings,
      this.root,
    );
    return {
      cues: cuePaths(changed, this.root),
      rev: changed.size ? rev : undefined,
      status: commitStatus(changed),
    };
  }

  setExpanded(path: string, open: boolean): void {
    ({ expanded: this.expanded, userClosed: this.userClosed } = toggleExpansion(
      this.expanded,
      this.userClosed,
      path,
      open,
    ));
    if (!open && this.selectedPath !== path && this.selectedPath.startsWith(path ? `${path}/` : "/")) {
      this.select(path);
    }
  }

  select(path: string): void {
    this.selectedPath = path;
  }

  loadSelected(path: string): void {
    this.selectedPath = path;
    this.loadEditor();
  }

  navigate(path: string, direction: NavDirection, step?: number): string {
    const next = movePath(this.visiblePaths, path, direction, step);
    this.select(next);
    return next;
  }

  updateEditor(value: string): void {
    this.editor = value;
  }

  loadEditor(): void {
    const node = this.selected;
    this.editor = node?.kind === "leaf" && node.present ? JSON.stringify(node.value, null, 2) : "null";
  }

  setFlashed(paths: Set<string>): void {
    this.flashed = paths;
  }

  parseEditor(): unknown {
    return JSON.parse(this.editor);
  }

  private rebuild(reloadEditor = true): void {
    this.tree = treeSnapshot(this.schema, this.root, this.settings);
    if (!this.tree.nodeByPath.has(this.selectedPath)) {
      this.selectedPath = this.tree.nodes[0]?.path ?? "";
    }
    if (reloadEditor) {
      this.loadEditor();
    }
  }
}

function commitStatus(changed: Set<string>): string {
  if (changed.size === 1) {
    return `Updated ${displayPath([...changed][0])}`;
  }
  return changed.size ? `Updated ${changed.size} settings` : "";
}

function emptyTree(): TreeSnapshot {
  return {
    nodes: [],
    flatNodes: new Map(),
    nodeViews: new Map(),
    nodeByPath: new Map(),
  };
}
