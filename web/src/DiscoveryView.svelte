<svelte:options runes={true} />

<script lang="ts">
  import { untrack } from "svelte";
  import type { MqttAuth } from "./lib/mqtt-session";
  import { discoveryTree } from "./lib/discovery-tree";
  import type { TreeActions, TreeNodeView } from "./lib/tree-view";
  import {
    movePath,
    visibleTreePaths,
    type NavDirection,
  } from "./lib/tree-navigation";
  import StatusLog from "./StatusLog.svelte";
  import TreeView from "./TreeView.svelte";
  import BuildIdentity from "./BuildIdentity.svelte";

  type Props = {
    broker?: string;
    discoveryFilter?: string;
    auth?: MqttAuth;
    discoveredPrefixes: { prefix: string }[];
    status: string;
    watching: boolean;
    error: string;
    logOpen?: boolean;
    logLines: string[];
    connect: (broker: string, filter: string, auth: MqttAuth) => void;
    submitLabel?: string;
    browseHref: (prefix: string) => string;
  };

  let {
    broker: appliedBroker = "",
    discoveryFilter: appliedFilter = "",
    auth,
    discoveredPrefixes,
    status,
    watching,
    error,
    logOpen = $bindable(false),
    logLines,
    connect,
    submitLabel = "Discover",
    browseHref,
  }: Props = $props();

  let broker = $state(untrack(() => appliedBroker));
  let discoveryFilter = $state(untrack(() => appliedFilter));
  let username = $state(untrack(() => auth?.username ?? ""));
  let password = $state(untrack(() => auth?.password ?? ""));
  $effect(() => {
    broker = appliedBroker;
    discoveryFilter = appliedFilter;
    username = auth?.username ?? "";
    password = auth?.password ?? "";
    credentialBroker = appliedBroker.trim();
    formError = "";
  });

  let selectedPath = $state("");
  let credentialBroker = $state(untrack(() => broker.trim()));
  let formError = $state("");
  const credentialSection = $derived(
    `section-broker${Array.from(new TextEncoder().encode(broker.trim()), (byte) => byte.toString(16).padStart(2, "0")).join("")}` as const,
  );
  $effect(() => {
    const next = broker.trim();
    if (next !== credentialBroker) {
      credentialBroker = next;
      username = password = "";
    }
  });
  let userClosed = $state(new Set<string>());
  let treeNodes = $derived(discoveryTree(discoveredPrefixes, browseHref));
  let expanded = $derived(
    new Set(
      [...treeNodes.values()]
        .filter((node) => node.children.length && !userClosed.has(node.path))
        .map((node) => node.path),
    ),
  );
  let visiblePaths = $derived(visibleTreePaths("", treeNodes, expanded));

  $effect(() => {
    if (!visiblePaths.includes(selectedPath)) {
      selectedPath = visiblePaths[0] ?? "";
    }
  });

  function select(path: string) {
    selectedPath = path;
  }

  function setExpanded(path: string, open: boolean) {
    userClosed = new Set(userClosed);
    if (open) userClosed.delete(path);
    else userClosed.add(path);
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
    key: (node: TreeNodeView, direction: NavDirection, step?: number) => {
      return navigateTree(node.path, direction, step);
    },
    open: setExpanded,
    select,
  } satisfies TreeActions);

  function submit(event: SubmitEvent) {
    event.preventDefault();
    // Autofill may update visible fields without input events. Submit their values.
    const form = event.currentTarget as HTMLFormElement;
    const data = new FormData(form);
    const nextBroker = (data.get("broker") as string).trim();
    if (nextBroker !== credentialBroker) {
      broker = credentialBroker = nextBroker;
      username = password = "";
      for (const name of ["username", "password"])
        (form.elements.namedItem(name) as HTMLInputElement).value = "";
      formError =
        "Broker changed. Enter credentials for this broker, then connect.";
      return;
    }
    formError = "";
    broker = nextBroker;
    discoveryFilter = data.get("discovery-filter") as string;
    username = data.get("username") as string;
    password = data.get("password") as string;
    try {
      connect(broker, discoveryFilter, { username, password });
    } catch (error) {
      formError = error instanceof Error ? error.message : String(error);
    }
  }
</script>

<section class="discovery">
  <section class="connection panel" aria-labelledby="connect-title">
    <header>
      <h1 id="connect-title">Miniconf Web</h1>
      <BuildIdentity />
      <p>Discover and inspect Miniconf devices on an MQTT broker.</p>
    </header>
    <!-- The shortcut handles bubbled keystrokes from the form's native controls. -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <form
      autocomplete="on"
      onsubmit={submit}
      onkeydown={(event) => {
        if (
          !event.isComposing &&
          !event.repeat &&
          (event.ctrlKey || event.metaKey) &&
          event.key === "Enter"
        ) {
          event.preventDefault();
          event.currentTarget.requestSubmit();
        }
      }}
    >
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
      <label class="filter">
        Discovery filter
        <input
          bind:value={discoveryFilter}
          name="discovery-filter"
          aria-describedby="filter-help"
        />
        <span id="filter-help" class="meta"
          >Prefix filter; /alive is appended. Use + for one level; # is
          unsupported.</span
        >
      </label>
      {#key credentialSection}
        <label>
          Username
          <input
            autocomplete={`${credentialSection} username`}
            bind:value={username}
            name="username"
          />
        </label>
        <label>
          Password
          <input
            autocomplete={`${credentialSection} current-password`}
            bind:value={password}
            name="password"
            type="password"
          />
        </label>
      {/key}
      <button
        type="submit"
        aria-keyshortcuts="Control+Enter Meta+Enter"
        title="Ctrl/Cmd+Enter">{submitLabel}</button
      >
    </form>
    <StatusLog
      {status}
      error={formError || error}
      bind:open={logOpen}
      {logLines}
      live
    />
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
    grid-column: 1 / -1;
  }

  .connection > header {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: baseline;
    gap: var(--space-tight) var(--space);
  }

  form {
    align-items: start;
    display: grid;
    gap: var(--space);
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
