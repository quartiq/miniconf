<script lang="ts">
  import { discoveryTree, discoveryTreeView, flatDiscoveryNodes } from "./lib/discovery-tree";
  import { TreeInteraction } from "./lib/tree-interaction";
  import type { TreeActions, TreeNodeView } from "./lib/tree-view";
  import { type NavDirection } from "./lib/tree-navigation";
  import TreeView from "./TreeView.svelte";

  export let broker: string;
  export let discoveryPattern: string;
  export let username: string;
  export let password: string;
  export let discoveredPrefixes: { prefix: string }[];
  export let discover: () => void;
  export let browseHref: (prefix: string) => string;

  let interaction = new TreeInteraction([""]);
  let prefixKey = "";

  $: nodes = discoveryTree(discoveredPrefixes);
  $: treeNodes = discoveryTreeView(nodes, browseHref);
  $: nextPrefixKey = discoveredPrefixes.map((prefix) => prefix.prefix).sort().join("\n");
  $: if (nextPrefixKey !== prefixKey) {
    prefixKey = nextPrefixKey;
    interaction.expanded = new Set([
      ...interaction.expanded,
      ...[...nodes.values()]
        .filter((node) => node.children.length && !interaction.userClosed.has(node.path))
        .map((node) => node.path),
    ]);
    interaction = interaction;
  }
  $: flatNodes = flatDiscoveryNodes(nodes);
  $: visiblePaths = interaction.visiblePaths("", flatNodes);
  $: interaction.ensureSelected(visiblePaths);
  $: selectedPath = interaction.selectedPath;
  $: expanded = interaction.expanded;

  function select(path: string) {
    interaction.select(path);
    interaction = interaction;
  }

  function setExpanded(path: string, open: boolean) {
    interaction.setExpanded(path, open);
    interaction = interaction;
  }

  function navigateTree(path: string, direction: NavDirection, step?: number): string {
    const next = interaction.navigate("", flatNodes, path, direction, step);
    interaction = interaction;
    return next;
  }

  $: treeActions = {
    activate: (node: TreeNodeView, internal: boolean, open: boolean) => {
      if (node.href) {
        location.href = node.href;
      } else if (internal) {
        setExpanded(node.path, !open);
      }
    },
    key: (node: TreeNodeView, direction: NavDirection, step?: number) => {
      return navigateTree(node.path, direction, step);
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
    <TreeView
      root=""
      nodes={treeNodes}
      {selectedPath}
      {expanded}
      actions={treeActions}
    />
  </section>
{/if}
