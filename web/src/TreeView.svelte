<svelte:options runes={true} />

<script lang="ts">
  import { tick } from "svelte";
  import type {
    TreeActions,
    TreeActivity,
    TreeNodeView,
  } from "./lib/tree-view";
  import {
    movePath,
    treeTabStop,
    visibleTreePaths,
  } from "./lib/tree-navigation";
  import type { NavDirection } from "./lib/tree-navigation";
  import TreeItem from "./TreeItem.svelte";

  type Props = {
    label: string;
    roots: string[];
    nodes: Map<string, TreeNodeView>;
    selectedPath: string;
    expanded: Set<string>;
    activity?: Map<string, TreeActivity>;
    actions: Omit<TreeActions, "key">;
  };

  let {
    label,
    roots,
    nodes,
    selectedPath,
    expanded,
    activity,
    actions,
  }: Props = $props();

  let tree: HTMLUListElement;
  let visible = $derived(
    roots.flatMap((root) => visibleTreePaths(root, nodes, expanded)),
  );
  let tabStop = $derived(treeTabStop(selectedPath, visible));
  export async function focus(path = selectedPath) {
    await tick();
    const row = tree?.querySelector<HTMLElement>(
      `[data-tree-path="${CSS.escape(treeTabStop(path, visible))}"]`,
    );
    (row ?? tree)?.focus({ preventScroll: true });
    row?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }
  $effect.pre(() => {
    const paths = visible;
    const active = document.activeElement as HTMLElement | null;
    if (!active || !tree?.contains(active)) return;
    const path =
      active.closest<HTMLElement>("[data-tree-path]")?.dataset.treePath;
    if (path === undefined || paths.includes(path)) return;
    void tick().then(() => {
      if (!active.isConnected && document.activeElement === document.body)
        void focus(path);
    });
  });
  let treeActions = $derived({
    ...actions,
    key(node: TreeNodeView, direction: NavDirection, step?: number) {
      const next = movePath(visible, node.path, direction, nodes, step);
      actions.select(next);
      void focus(next);
      return next;
    },
  } satisfies TreeActions);
</script>

<ul role="tree" aria-label={label} tabindex="-1" bind:this={tree}>
  {#each roots as root, index (root)}
    {@const rootNode = nodes.get(root)}
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
        index={index + 1}
        size={roots.length}
      />
    {/if}
  {/each}
</ul>

<style>
  ul {
    margin: 0;
    min-width: 0;
    padding: 0;
  }
</style>
