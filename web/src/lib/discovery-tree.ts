import type { TreeNodeView } from "./tree-view";

type PrefixEntry = {
  prefix: string;
};

export function discoveryTree(
  prefixes: PrefixEntry[],
  browseHref: (prefix: string) => string,
): Map<string, TreeNodeView> {
  const nodes = new Map<string, TreeNodeView>([
    ["", { path: "", label: "prefixes", children: [] }],
  ]);

  for (const discovered of prefixes) {
    let parent = nodes.get("")!;
    for (const segment of discovered.prefix.split("/")) {
      // Row keys add one slash to the literal MQTT prefix. This distinguishes
      // the synthetic root from an empty first level; wire topics stay unchanged.
      const path = `${parent.path}/${segment}`;
      let node = nodes.get(path);
      if (!node) {
        node = {
          path,
          label: segment || '""',
          title: path.slice(1) || '""',
          parent: parent.path,
          children: [],
        };
        nodes.set(path, node);
        parent.children.push(path);
      }
      parent = node;
    }
    parent.href = browseHref(discovered.prefix);
  }

  return nodes;
}
