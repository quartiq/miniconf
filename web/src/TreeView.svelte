<svelte:options runes={true} />

<script lang="ts">
  import type {
    TreeActions,
    TreeActivity,
    TreeNodeView,
  } from "./lib/tree-view";
  import { treeTabStop, visibleTreePaths } from "./lib/tree-navigation";
  import type { NavDirection } from "./lib/tree-navigation";
  import TreeItem from "./TreeItem.svelte";

  type Props = {
    label: string;
    root: string;
    nodes: Map<string, TreeNodeView>;
    selectedPath: string;
    expanded: Set<string>;
    activity?: Map<string, TreeActivity>;
    actions: TreeActions;
  };

  let { label, root, nodes, selectedPath, expanded, activity, actions }: Props =
    $props();

  let tabStop = $derived(
    treeTabStop(selectedPath, visibleTreePaths(root, nodes, expanded)),
  );
  let rootNode = $derived(nodes.get(root));
  let treeActions = $derived({
    ...actions,
    key(node: TreeNodeView, direction: NavDirection, step?: number) {
      const next = actions.key(node, direction, step);
      requestAnimationFrame(() => {
        const row = document.querySelector<HTMLElement>(
          `[data-tree-path="${CSS.escape(next)}"]`,
        );
        row?.focus({ preventScroll: true });
        row?.scrollIntoView({ block: "nearest", inline: "nearest" });
      });
      return next;
    },
  } satisfies TreeActions);
</script>

<ul role="tree" aria-label={label}>
  {#if rootNode}
    <TreeItem
      node={rootNode}
      {nodes}
      {selectedPath}
      {tabStop}
      {activity}
      showActivity={activity !== undefined}
      {expanded}
      actions={treeActions}
    />
  {/if}
</ul>

<style>
  ul {
    margin: 0;
    min-width: 0;
    padding: 0;
  }
</style>
