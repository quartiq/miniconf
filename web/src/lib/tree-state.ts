import type { Schema, SchemaNode } from "./schema";
import { formatSchemaName, schemaSummary, schemaTooltip } from "./schema";
import {
  ACTIVITY_DURATION_MS,
  type TreeActivity,
  type TreeNodeView,
} from "./tree-view";

export type ViewNode = TreeNodeView &
  Pick<SchemaNode, "kind" | "node" | "edge" | "sem">;

export function updateActivity(
  previous: Map<string, TreeActivity>,
  cues: Iterable<string>,
  tree: Map<string, ViewNode>,
  at = Date.now(),
): Map<string, TreeActivity> {
  const next = new Map<string, TreeActivity>();
  for (const [path, activity] of previous) {
    if (tree.has(path) && at - activity.at < ACTIVITY_DURATION_MS)
      next.set(path, activity);
  }
  const activity = { at };
  for (const path of cues) if (tree.has(path)) next.set(path, activity);
  return next;
}

export function parentPath(path: string): string | undefined {
  if (!path) {
    return undefined;
  }
  const index = path.lastIndexOf("/");
  return index <= 0 ? "" : path.slice(0, index);
}

export function treeSnapshot(
  schema: Schema | undefined,
  root: string,
  settings: Map<string, string>,
): Map<string, ViewNode> {
  return new Map(
    (schema?.walk(root) ?? []).map((node) => [
      node.path,
      {
        ...node,
        parent: parentPath(node.path),
        label: node.path ? formatSchemaName(node) : "(root)",
        summary: schemaSummary(node),
        title: schemaTooltip(node),
        value: node.kind === "leaf" ? settings.get(node.path) : undefined,
        children: node.children.map((child) => child.path),
      },
    ]),
  );
}

export function revealPresentSettings(
  expanded: Set<string>,
  userClosed: Set<string>,
  changed: Iterable<string>,
  settings: Map<string, string>,
  root: string,
): Set<string> {
  let next = expanded;
  for (const path of changed) {
    if (!settings.has(path)) {
      continue;
    }
    let parent = parentPath(path);
    while (parent !== undefined) {
      // Auto-reveal only for branches the user has not explicitly closed;
      // retained startup bursts must not fight manual folding.
      if (
        !next.has(parent) &&
        withinRoot(parent, root) &&
        autoExpandAllowed(parent, userClosed)
      ) {
        if (next === expanded) next = new Set(expanded);
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

function autoExpandAllowed(path: string, userClosed: Set<string>): boolean {
  for (const item of userClosed) {
    if (path === item || path.startsWith(`${item}/`)) {
      return false;
    }
  }
  return true;
}
