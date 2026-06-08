<script lang="ts">
  import { discoveryTree, discoveryTreeView, flatDiscoveryNodes } from "./lib/discovery-tree";
  import type { TreeActions, TreeNodeView } from "./lib/tree-view";
  import { movePath, toggleExpansion, visibleTreePaths, type NavDirection } from "./lib/tree-navigation";
  import TreeItem from "./TreeItem.svelte";

  export let broker: string;
  export let discoveryPattern: string;
  export let username: string;
  export let password: string;
  export let discoveredPrefixes: { prefix: string }[];
  export let discover: () => void;
  export let browseHref: (prefix: string) => string;

  let selectedPath = "";
  let expanded = new Set([""]);
  let userClosed = new Set<string>();
  let prefixKey = "";

  $: nodes = discoveryTree(discoveredPrefixes);
  $: treeNodes = discoveryTreeView(nodes, browseHref);
  $: rootNode = nodes.get("");
  $: nextPrefixKey = discoveredPrefixes.map((prefix) => prefix.prefix).sort().join("\n");
  $: if (nextPrefixKey !== prefixKey) {
    prefixKey = nextPrefixKey;
    expanded = new Set([
      ...expanded,
      ...[...nodes.values()]
        .filter((node) => node.children.length && !userClosed.has(node.path))
        .map((node) => node.path),
    ]);
  }
  $: visiblePaths = visibleTreePaths("", flatDiscoveryNodes(nodes), expanded);
  $: if (!nodes.has(selectedPath)) {
    selectedPath = visiblePaths[0] ?? "";
  }

  function select(path: string) {
    selectedPath = path;
  }

  function setExpanded(path: string, open: boolean) {
    ({ expanded, userClosed } = toggleExpansion(expanded, userClosed, path, open));
  }

  function focusTreeItem(path: string) {
    requestAnimationFrame(() => {
      document
        .querySelector<HTMLElement>(`[data-tree-path="${CSS.escape(path)}"]`)
        ?.focus();
    });
  }

  function navigateTree(path: string, direction: NavDirection, step?: number) {
    const next = movePath(visiblePaths, path, direction, step);
    select(next);
    focusTreeItem(next);
  }

  $: treeActions = {
    key: (node: TreeNodeView, direction: NavDirection, step?: number) => {
      navigateTree(node.path, direction, step);
    },
    open: setExpanded,
    select,
  } satisfies TreeActions;
</script>

<header>
  <h1>Discover Prefixes</h1>
  <form on:submit|preventDefault={discover}>
    <label>
      Broker
      <input bind:value={broker} />
    </label>
    <label>
      Pattern
      <input bind:value={discoveryPattern} />
    </label>
    <label>
      Username
      <input autocomplete="username" bind:value={username} />
    </label>
    <label>
      Password
      <input autocomplete="current-password" bind:value={password} type="password" />
    </label>
    <button type="submit">Discover</button>
  </form>
</header>

{#if discoveredPrefixes.length}
  <section>
    <h2>Prefixes</h2>
    <ul role="tree">
      {#if rootNode}
        <TreeItem
          node={treeNodes.get("")!}
          nodes={treeNodes}
          {selectedPath}
          {expanded}
          actions={treeActions}
        />
      {/if}
    </ul>
  </section>
{/if}

<style>
  ul {
    margin: 0;
    padding: 0;
  }
</style>
