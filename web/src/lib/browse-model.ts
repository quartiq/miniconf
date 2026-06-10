import { type Schema } from "./schema";
import type { Settings } from "./settings-mirror";
import {
  cuePaths,
  revealPresentSettings,
  treeSnapshot,
  type TreeSnapshot,
  type ViewNode,
} from "./tree-state";
import { TreeInteraction } from "./tree-interaction";
import { type NavDirection } from "./tree-navigation";

// Pure browse UI state. The editor draft is user-owned after selection/opening;
// incoming settings rebuild row values and flashes but must not overwrite it.
export type BrowseCommit = {
  cues: Set<string>;
  rev?: string;
};

type BrowseSettings = {
  settings: Settings;
  changed: Set<string>;
  rev?: string;
};

export class BrowseModel {
  schema: Schema | undefined;
  settings: Settings = new Map();
  root = "";
  editor = "null";
  flashed = new Set<string>();
  interaction = new TreeInteraction();
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
    return this.interaction.visiblePaths(this.root, this.tree.flatNodes);
  }

  get expanded(): Set<string> {
    return this.interaction.expanded;
  }

  get selectedPath(): string {
    return this.interaction.selectedPath;
  }

  reset(): void {
    this.schema = undefined;
    this.settings = new Map();
    this.root = "";
    this.tree = emptyTree();
    this.interaction.reset();
    this.editor = "null";
    this.flashed = new Set();
  }

  loadSchema(schema: Schema, subtreePath: string): string {
    this.schema = schema;
    this.root = schema.path(subtreePath);
    this.settings = new Map();
    this.interaction.reset();
    this.rebuild();
    return this.root;
  }

  commit({ settings, changed, rev }: BrowseSettings): BrowseCommit {
    this.settings = settings;
    this.rebuild(false);
    this.interaction.expanded = revealPresentSettings(
      this.interaction.expanded,
      this.interaction.userClosed,
      changed,
      this.settings,
      this.root,
    );
    return {
      cues: cuePaths(changed, this.root),
      rev: changed.size ? rev : undefined,
    };
  }

  setExpanded(path: string, open: boolean): void {
    this.interaction.setExpanded(path, open);
    if (!open && this.selectedPath !== path && this.selectedPath.startsWith(path ? `${path}/` : "/")) {
      this.select(path);
    }
  }

  select(path: string): void {
    this.interaction.select(path);
  }

  loadSelected(path: string): void {
    this.interaction.select(path);
    this.loadEditor();
  }

  navigate(path: string, direction: NavDirection, step?: number): string {
    return this.interaction.navigate(this.root, this.tree.flatNodes, path, direction, step);
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
    this.interaction.ensureSelected(this.tree.nodes.map((node) => node.path));
    if (reloadEditor) {
      this.loadEditor();
    }
  }
}

function emptyTree(): TreeSnapshot {
  return {
    nodes: [],
    flatNodes: new Map(),
    nodeViews: new Map(),
    nodeByPath: new Map(),
  };
}
