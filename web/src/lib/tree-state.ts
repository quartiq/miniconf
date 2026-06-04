import type { Schema, SchemaNode } from "./schema";
import { formatSchemaName } from "./schema";
import type { FlatTreeNode } from "./tree-navigation";
import type { TreeNodeView } from "./tree-view";

export type ViewNode = SchemaNode & {
  value?: unknown;
  present: boolean;
};

export function parentPath(path: string): string | undefined {
  if (!path) {
    return undefined;
  }
  const index = path.lastIndexOf("/");
  return index <= 0 ? "" : path.slice(0, index);
}

export function viewNodes(schema: Schema | undefined, root: string, settings: Map<string, unknown>): ViewNode[] {
  return schema?.walk(root).map((node) => ({
    ...node,
    value: settings.get(node.path),
    present: settings.has(node.path),
  })) ?? [];
}

export function formatLeafValue(node: ViewNode): string {
  if (node.kind !== "leaf" || !node.present) {
    return "";
  }
  return JSON.stringify(node.value) ?? String(node.value);
}

export function childrenByParent(nodes: ViewNode[]): Map<string, ViewNode[]> {
  const children = new Map<string, ViewNode[]>();
  for (const node of nodes) {
    const parent = parentPath(node.path);
    if (parent !== undefined) {
      const siblings = children.get(parent);
      if (siblings) {
        siblings.push(node);
      } else {
        children.set(parent, [node]);
      }
    }
  }
  return children;
}

export function flatTreeNodes(nodes: ViewNode[]): Map<string, FlatTreeNode> {
  const children = childrenByParent(nodes);
  return new Map(nodes.map((node) => [
    node.path,
    {
      path: node.path,
      children: (children.get(node.path) ?? []).map((child) => child.path),
    },
  ]));
}

export function treeViewNodes(
  nodes: ViewNode[],
  root: string,
): Map<string, TreeNodeView> {
  const children = childrenByParent(nodes);
  return new Map(nodes.map((node) => [
    node.path,
    {
      path: node.path,
      // The selected subtree root is the empty path label in the UI, not a
      // synthetic "/settings" or "Root" name. The caret already communicates
      // that it is foldable.
      label: formatSchemaName(node, node.path === root ? "" : undefined),
      value: formatLeafValue(node),
      children: (children.get(node.path) ?? []).map((child) => child.path),
    },
  ]));
}

export function revealPresentSettings(
  expanded: Set<string>,
  collapsed: Set<string>,
  changed: Iterable<string>,
  settings: Map<string, unknown>,
  root: string,
): Set<string> {
  const next = new Set(expanded);
  for (const path of changed) {
    if (!settings.has(path)) {
      continue;
    }
    let parent = parentPath(path);
    while (parent !== undefined) {
      // Auto-reveal only for branches the user has not explicitly collapsed;
      // retained startup bursts must not fight manual folding.
      if (withinRoot(parent, root) && autoExpandAllowed(parent, collapsed)) {
        next.add(parent);
      }
      if (parent === root) {
        break;
      }
      parent = parentPath(parent);
    }
  }
  return next;
}

export function cuePaths(paths: Iterable<string>, root: string): Set<string> {
  const cued = new Set<string>();
  for (const path of paths) {
    // Flash the changed leaf and visible ancestors so updates are findable even
    // when a subtree is folded.
    cued.add(path);
    let parent = parentPath(path);
    while (parent !== undefined) {
      cued.add(parent);
      if (parent === root) {
        break;
      }
      parent = parentPath(parent);
    }
  }
  return cued;
}

function withinRoot(path: string, root: string): boolean {
  return !root || path === root || path.startsWith(`${root}/`);
}

function autoExpandAllowed(path: string, collapsed: Set<string>): boolean {
  for (const item of collapsed) {
    if (path === item || path.startsWith(`${item}/`)) {
      return false;
    }
  }
  return true;
}
