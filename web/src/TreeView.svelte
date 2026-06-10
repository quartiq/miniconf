<svelte:options runes={true} />

<script lang="ts">
  import type { TreeActions, TreeNodeView } from "./lib/tree-view";
  import type { NavDirection } from "./lib/tree-navigation";
  import TreeItem from "./TreeItem.svelte";

  type Props = {
    root: string;
    nodes: Map<string, TreeNodeView>;
    selectedPath: string;
    expanded: Set<string>;
    flashed?: Set<string>;
    actions: TreeActions;
  };

  let {
    root,
    nodes,
    selectedPath,
    expanded,
    flashed = new Set(),
    actions,
  }: Props = $props();

  let rootNode = $derived(nodes.get(root));
  let focusPath = $state<string | undefined>();
  let treeActions = $derived({
    ...actions,
    key(node: TreeNodeView, direction: NavDirection, step?: number) {
      const next = actions.key(node, direction, step);
      focusPath = next;
      return next;
    },
  } satisfies TreeActions);

  $effect(() => {
    if (focusPath === undefined) {
      return;
    }
    requestAnimationFrame(() => {
      document
        .querySelector<HTMLElement>(`[data-tree-path="${CSS.escape(focusPath)}"]`)
        ?.focus();
    });
  });
</script>

<ul role="tree">
  {#if rootNode}
    <TreeItem
      node={rootNode}
      {nodes}
      {selectedPath}
      {flashed}
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
