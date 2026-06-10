<svelte:options runes={true} />

<script lang="ts">
  import { discoveryTree, discoveryTreeView, flatDiscoveryNodes } from "./lib/discovery-tree";
  import type { TreeActions, TreeNodeView } from "./lib/tree-view";
  import {
    movePath,
    toggleExpansion,
    visibleTreePaths,
    type NavDirection,
  } from "./lib/tree-navigation";
  import TreeView from "./TreeView.svelte";

  type Props = {
    broker?: string;
    discoveryPattern?: string;
    username?: string;
    password?: string;
    discoveredPrefixes: { prefix: string }[];
    discover: () => void;
    browseHref: (prefix: string) => string;
  };

  let {
    broker = $bindable(""),
    discoveryPattern = $bindable(""),
    username = $bindable(""),
    password = $bindable(""),
    discoveredPrefixes,
    discover,
    browseHref,
  }: Props = $props();

  let expanded = $state(new Set([""]));
  let selectedPath = $state("");
  let userClosed = $state(new Set<string>());
  let prefixKey = $state("");
  let nodes = $derived(discoveryTree(discoveredPrefixes));
  let treeNodes = $derived(discoveryTreeView(nodes, browseHref));
  let flatNodes = $derived(flatDiscoveryNodes(nodes));
  let visiblePaths = $derived(visibleTreePaths("", flatNodes, expanded));
  let nextPrefixKey = $derived(discoveredPrefixes.map((prefix) => prefix.prefix).sort().join("\n"));

  $effect(() => {
    if (nextPrefixKey === prefixKey) {
      return;
    }
    prefixKey = nextPrefixKey;
    expanded = new Set([
      ...expanded,
      ...[...nodes.values()]
        .filter((node) => node.children.length && !userClosed.has(node.path))
        .map((node) => node.path),
    ]);
  });

  $effect(() => {
    if (!visiblePaths.includes(selectedPath)) {
      selectedPath = visiblePaths[0] ?? "";
    }
  });

  function select(path: string) {
    selectedPath = path;
  }

  function setExpanded(path: string, open: boolean) {
    ({ expanded, userClosed } = toggleExpansion(expanded, userClosed, path, open));
  }

  function navigateTree(path: string, direction: NavDirection, step?: number): string {
    const next = movePath(visiblePaths, path, direction, flatNodes, step);
    selectedPath = next;
    return next;
  }

  let treeActions = $derived({
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
  } satisfies TreeActions);

  function submit(event: SubmitEvent) {
    event.preventDefault();
    discover();
  }
</script>

<header>
  <h1>Discover Prefixes</h1>
  <form onsubmit={submit}>
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
