import type { FlatTreeNode } from "./tree-navigation";
import type { TreeNodeView } from "./tree-view";

type PrefixEntry = {
  prefix: string;
};

export type DiscoveryNode = {
  path: string;
  label: string;
  prefix?: string;
  children: string[];
};

export function discoveryTree(prefixes: PrefixEntry[]): Map<string, DiscoveryNode> {
  const nodes = new Map<string, DiscoveryNode>([
    ["", { path: "", label: "prefixes", children: [] }],
  ]);

  function ensure(path: string, label: string): DiscoveryNode {
    let node = nodes.get(path);
    if (!node) {
      node = { path, label, children: [] };
      nodes.set(path, node);
    }
    return node;
  }

  for (const discovered of prefixes) {
    let parent = "";
    for (const segment of discovered.prefix.split("/")) {
      const path = parent ? `${parent}/${segment}` : segment;
      const node = ensure(path, segment);
      const parentNode = ensure(parent, parent ? parent.split("/").at(-1)! : "prefixes");
      if (!parentNode.children.includes(path)) {
        parentNode.children.push(path);
      }
      parent = node.path;
    }
    nodes.get(parent)!.prefix = discovered.prefix;
  }

  return nodes;
}

export function flatDiscoveryNodes(nodes: Map<string, DiscoveryNode>): Map<string, FlatTreeNode> {
  return new Map([...nodes].map(([path, node]) => [path, { path, children: node.children }]));
}

export function discoveryTreeView(
  nodes: Map<string, DiscoveryNode>,
  browseHref: (prefix: string) => string,
): Map<string, TreeNodeView> {
  return new Map([...nodes].map(([path, node]) => [
    path,
    {
      path,
      label: node.label,
      href: node.prefix ? browseHref(node.prefix) : undefined,
      children: node.children,
    },
  ]));
}
