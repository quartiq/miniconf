import type { FlatTreeNode, NavDirection } from "./tree-navigation";

// Shared tree contract for discovery and browsing. Row clicks select only;
// caret/keyboard own folding, and view-specific activation is optional.
export type TreeNodeView = FlatTreeNode & {
  label: string;
  summary?: string;
  title?: string;
  value?: string;
  href?: string;
};

export type TreeActivity = {
  at: number;
};

export type TreeActions = {
  activate?: (node: TreeNodeView, internal: boolean, open: boolean) => void;
  key: (node: TreeNodeView, direction: NavDirection, step?: number) => string;
  open: (path: string, open: boolean) => void;
  select: (path: string) => void;
};
