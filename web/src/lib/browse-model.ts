import { displayPath, type Schema } from "./schema";
import type { Settings } from "./settings-mirror";
import {
  cuePaths,
  flatTreeNodes,
  revealPresentSettings,
  treeViewNodes,
  viewNodes,
  type ViewNode,
} from "./tree-state";
import { movePath, toggleExpansion, visibleTreePaths, type NavDirection } from "./tree-navigation";
import type { FlatTreeNode } from "./tree-navigation";
import type { TreeNodeView } from "./tree-view";

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
  private collapsed = new Set<string>();

  schema: Schema | undefined;
  settings: Settings = new Map();
  root = "";
  nodes: ViewNode[] = [];
  expanded = new Set<string>();
  selectedPath = "";
  editor = "null";
  flashed = new Set<string>();
  private flatNodes = new Map<string, FlatTreeNode>();
  private nodeViews = new Map<string, TreeNodeView>();
  private nodeByPath = new Map<string, ViewNode>();

  get selected(): ViewNode | undefined {
    return this.nodeByPath.get(this.selectedPath);
  }

  get rootNode(): ViewNode | undefined {
    return this.nodeByPath.get(this.root);
  }

  get treeNodes() {
    return this.nodeViews;
  }

  get visiblePaths(): string[] {
    return visibleTreePaths(this.root, this.flatNodes, this.expanded);
  }

  reset(): void {
    this.collapsed = new Set();
    this.schema = undefined;
    this.settings = new Map();
    this.root = "";
    this.nodes = [];
    this.flatNodes = new Map();
    this.nodeViews = new Map();
    this.nodeByPath = new Map();
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
    this.collapsed = new Set();
    this.rebuild();
    return this.root;
  }

  commit({ settings, changed, rev }: BrowseSettings): BrowseCommit {
    this.settings = settings;
    this.rebuild(false);
    this.expanded = revealPresentSettings(
      this.expanded,
      this.collapsed,
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
    ({ expanded: this.expanded, collapsed: this.collapsed } = toggleExpansion(
      this.expanded,
      this.collapsed,
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
    this.nodes = viewNodes(this.schema, this.root, this.settings);
    this.flatNodes = flatTreeNodes(this.nodes);
    this.nodeViews = treeViewNodes(this.nodes, this.root);
    this.nodeByPath = new Map(this.nodes.map((node) => [node.path, node]));
    if (!this.nodeByPath.has(this.selectedPath)) {
      this.selectedPath = this.nodes[0]?.path ?? "";
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
