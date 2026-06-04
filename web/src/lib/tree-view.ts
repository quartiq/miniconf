import type { NavDirection } from "./tree-navigation";

// Shared tree contract for discovery and browsing. Row clicks select only;
// caret/keyboard own folding, and view-specific activation is optional.
export type TreeNodeView = {
  path: string;
  label: string;
  value?: string;
  href?: string;
  children: string[];
};

export type TreeActions = {
  activate?: (node: TreeNodeView, internal: boolean, open: boolean) => void;
  key: (node: TreeNodeView, direction: NavDirection, step?: number) => void;
  open: (path: string, open: boolean) => void;
  select: (path: string) => void;
};
