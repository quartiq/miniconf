<svelte:options runes={true} />

<script lang="ts">
  import { tick } from "svelte";
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

  let tree: HTMLUListElement;
  let visible = $derived(visibleTreePaths(root, nodes, expanded));
  let tabStop = $derived(treeTabStop(selectedPath, visible));
  export async function focus(path = selectedPath) {
    await tick();
    const row = tree?.querySelector<HTMLElement>(
      `[data-tree-path="${CSS.escape(treeTabStop(path, visible))}"]`,
    );
    row?.focus({ preventScroll: true });
    row?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }
  $effect.pre(() => {
    const paths = visible;
    const active = document.activeElement as HTMLElement | null;
    if (!active || !tree?.contains(active)) return;
    const path = active.dataset.treePath;
    if (path === undefined || paths.includes(path)) return;
    void tick().then(() => {
      if (!active.isConnected && document.activeElement === document.body)
        void focus(path);
    });
  });
  let rootNode = $derived(nodes.get(root));
  let treeActions = $derived({
    ...actions,
    key(node: TreeNodeView, direction: NavDirection, step?: number) {
      const next = actions.key(node, direction, step);
      void focus(next);
      return next;
    },
  } satisfies TreeActions);
</script>

<ul role="tree" aria-label={label} bind:this={tree}>
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
