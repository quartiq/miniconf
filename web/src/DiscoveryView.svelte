<svelte:options runes={true} />

<script lang="ts">
  import { discoveryTree, discoveryTreeView } from "./lib/discovery-tree";
  import type { TreeActions, TreeNodeView } from "./lib/tree-view";
  import {
    movePath,
    toggleExpansion,
    visibleTreePaths,
    type NavDirection,
  } from "./lib/tree-navigation";
  import StatusLog from "./StatusLog.svelte";
  import TreeView from "./TreeView.svelte";

  type Props = {
    broker?: string;
    discoveryPattern?: string;
    username?: string;
    password?: string;
    discoveredPrefixes: { prefix: string }[];
    status: string;
    watching: boolean;
    error: string;
    logOpen?: boolean;
    logLines: string[];
    discover: () => void;
    browseHref: (prefix: string) => string;
  };

  let {
    broker = $bindable(""),
    discoveryPattern = $bindable(""),
    username = $bindable(""),
    password = $bindable(""),
    discoveredPrefixes,
    status,
    watching,
    error,
    logOpen = $bindable(false),
    logLines,
    discover,
    browseHref,
  }: Props = $props();

  let expanded = $state(new Set([""]));
  let selectedPath = $state("");
  let userClosed = $state(new Set<string>());
  let prefixKey = $state("");
  let nodes = $derived(discoveryTree(discoveredPrefixes));
  let treeNodes = $derived(discoveryTreeView(nodes, browseHref));
  let visiblePaths = $derived(visibleTreePaths("", treeNodes, expanded));
  let nextPrefixKey = $derived(
    discoveredPrefixes
      .map((prefix) => prefix.prefix)
      .sort()
      .join("\n"),
  );

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
    ({ expanded, userClosed } = toggleExpansion(
      expanded,
      userClosed,
      path,
      open,
    ));
  }

  function navigateTree(
    path: string,
    direction: NavDirection,
    step?: number,
  ): string {
    const next = movePath(visiblePaths, path, direction, treeNodes, step);
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

<section class="discovery">
  <section class="connection panel" aria-labelledby="connect-title">
    <header>
      <h1 id="connect-title">Miniconf Browser</h1>
      <p>Discover and inspect Miniconf devices on an MQTT broker.</p>
    </header>
    <form autocomplete="on" onsubmit={submit}>
      <label class="broker">
        Broker
        <input
          autocomplete="url"
          bind:value={broker}
          name="broker"
          placeholder="wss://broker.example:443/path/to/socket"
          required
          type="url"
        />
      </label>
      <label class="pattern">
        Discovery filter
        <input
          bind:value={discoveryPattern}
          name="discovery-pattern"
          aria-describedby="filter-help"
        />
        <span id="filter-help" class="meta"
          >Prefix filter; /alive is appended. Use + for one level; # is
          unsupported.</span
        >
      </label>
      <label>
        Username
        <input autocomplete="username" bind:value={username} name="username" />
      </label>
      <label>
        Password
        <input
          autocomplete="current-password"
          bind:value={password}
          name="password"
          type="password"
        />
      </label>
      <button type="submit">Discover</button>
    </form>
    <StatusLog {status} {error} bind:open={logOpen} {logLines} live />
  </section>

  {#if discoveredPrefixes.length || watching}
    <section class="prefixes panel" aria-labelledby="prefix-title">
      <header class="section-heading">
        <h2 id="prefix-title">Devices</h2>
        <span class="meta">{discoveredPrefixes.length} found</span>
      </header>
      <TreeView
        label="Devices"
        root=""
        nodes={treeNodes}
        {selectedPath}
        {expanded}
        actions={treeActions}
      />
      {#if !discoveredPrefixes.length}
        <p class="meta">No matching devices announced.</p>
      {/if}
    </section>
  {/if}
</section>

<style>
  .discovery {
    display: grid;
    gap: var(--space);
    margin: clamp(1rem, 8svh, 5rem) auto 0;
    max-width: 58rem;
    width: 100%;
  }

  .connection {
    display: grid;
    gap: var(--space);
  }

  header p {
    color: var(--muted);
  }

  form {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  input {
    width: 100%;
  }

  button {
    grid-column: 1 / -1;
    justify-self: end;
  }

  .section-heading {
    align-items: baseline;
    display: flex;
    justify-content: space-between;
  }

  @media (max-width: 760px) {
    .discovery {
      margin-top: 0;
    }

    form {
      grid-template-columns: 1fr;
    }

    button {
      justify-self: stretch;
    }
  }
</style>
