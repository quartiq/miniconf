import {
  movePath,
  toggleExpansion,
  visibleTreePaths,
  type FlatTreeNode,
  type NavDirection,
} from "./tree-navigation";

export class TreeInteraction {
  expanded = new Set<string>();
  selectedPath = "";
  userClosed = new Set<string>();

  constructor(open: Iterable<string> = []) {
    this.expanded = new Set(open);
  }

  reset(open: Iterable<string> = []): void {
    this.expanded = new Set(open);
    this.selectedPath = "";
    this.userClosed = new Set();
  }

  ensureSelected(paths: string[]): void {
    if (!paths.includes(this.selectedPath)) {
      this.selectedPath = paths[0] ?? "";
    }
  }

  select(path: string): void {
    this.selectedPath = path;
  }

  setExpanded(path: string, open: boolean): void {
    ({ expanded: this.expanded, userClosed: this.userClosed } = toggleExpansion(
      this.expanded,
      this.userClosed,
      path,
      open,
    ));
  }

  visiblePaths(root: string, nodes: Map<string, FlatTreeNode>): string[] {
    return visibleTreePaths(root, nodes, this.expanded);
  }

  navigate(root: string, nodes: Map<string, FlatTreeNode>, path: string, direction: NavDirection, step?: number): string {
    const visible = this.visiblePaths(root, nodes);
    const next = movePath(visible, path, direction, nodes, step);
    this.select(next);
    return next;
  }
}
