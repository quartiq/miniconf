export type NavDirection =
  | "child"
  | "first"
  | "last"
  | "next"
  | "pageNext"
  | "pagePrevious"
  | "parent"
  | "previous";

export type FlatTreeNode = {
  path: string;
  parent?: string;
  children: string[];
};

export function visibleTreePaths(
  root: string,
  nodes: Map<string, FlatTreeNode>,
  expanded: Set<string>,
): string[] {
  const visible: string[] = [];

  function visit(path: string) {
    visible.push(path);
    const node = nodes.get(path);
    if (!node || !expanded.has(path)) {
      return;
    }
    for (const child of node.children) {
      visit(child);
    }
  }

  if (nodes.has(root)) {
    visit(root);
  }
  return visible;
}

export function movePath(
  visible: string[],
  current: string,
  direction: NavDirection,
  nodes: Map<string, FlatTreeNode>,
  step = 1,
): string {
  if (!visible.length) {
    return current;
  }
  const index = Math.max(0, visible.indexOf(current));
  switch (direction) {
    case "child": {
      const child = visible[index + 1];
      return child && nodes.get(child)?.parent === current ? child : current;
    }
    case "first":
      return visible[0];
    case "last":
      return visible[visible.length - 1];
    case "next":
      return visible[Math.min(index + 1, visible.length - 1)];
    case "pageNext":
      return visible[Math.min(index + Math.max(1, step), visible.length - 1)];
    case "pagePrevious":
      return visible[Math.max(index - Math.max(1, step), 0)];
    case "previous":
      return visible[Math.max(index - 1, 0)];
    case "parent": {
      const parent = nodes.get(current)?.parent;
      return parent !== undefined && visible.includes(parent) ? parent : current;
    }
  }
}

export function toggleExpansion(
  expanded: Set<string>,
  userClosed: Set<string>,
  path: string,
  open: boolean,
): { expanded: Set<string>; userClosed: Set<string> } {
  const nextExpanded = new Set(expanded);
  const nextUserClosed = new Set(userClosed);
  if (open) {
    nextExpanded.add(path);
    nextUserClosed.delete(path);
  } else {
    nextExpanded.delete(path);
    nextUserClosed.add(path);
  }
  return { expanded: nextExpanded, userClosed: nextUserClosed };
}
