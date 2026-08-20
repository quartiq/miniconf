<svelte:options runes={true} />

<script lang="ts">
  import { onMount } from "svelte";
  import { displayPath, type Schema } from "./lib/schema";
  import BrowseView from "./BrowseView.svelte";
  import DiscoveryView from "./DiscoveryView.svelte";
  import {
    MiniconfBackend,
    type PrefixSession,
    type DiscoveredPrefix,
    type AliveManifest,
  } from "./lib/backend";
  import * as browse from "./lib/browse-model";
  import { EventLog } from "./lib/event-log";
  import { browsePath, discoveryPath, readRoute } from "./lib/routes";
  import { type SettingsCommit } from "./lib/settings-mirror";
  import { type NavDirection } from "./lib/tree-navigation";
  import { type TreeActivity } from "./lib/tree-view";

  const route = readRoute(location);
  let broker = $state(route.broker);
  let discoveryPattern = $state(route.discoveryPattern);
  let activePrefix = $state(route.activePrefix);
  let subtreePath = $state(route.subtreePath);
  let username = $state("");
  let password = $state("");

  let backend: MiniconfBackend | undefined;
  let prefixSession: PrefixSession | undefined;
  let discoveredPrefixes = $state<DiscoveredPrefix[]>([]);
  let aliveManifest = $state<AliveManifest | undefined>();
  let browseState = $state(browse.emptyState());
  let status = $state("Idle");
  let settingsRevision = $state("");
  let error = $state("");
  let logOpen = $state(new URLSearchParams(location.search).get("log") === "1");
  let logLines = $state<string[]>([]);
  let stopConnection: (() => void) | undefined;
  let discoveryWatch: ReturnType<MiniconfBackend["watchDiscovery"]> | undefined;
  let routeSerial = 0;
  // Row flashes are UI cues for /settings echoes only. /set responses update
  // the status/log, but the retained/live settings mirror is authoritative.
  let treeActivity = $state.raw(new Map<string, TreeActivity>());
  const eventLog = new EventLog(() => {
    logLines = eventLog.lines;
  });

  let selected = $derived(browse.selected(browseState));
  let mode = $derived(activePrefix ? "browse" : "discover");

  $effect(() => {
    eventLog.clearHidden(logOpen);
  });

  function syncUrl() {
    history.replaceState(
      null,
      "",
      activePrefix
        ? browsePath(broker, activePrefix, subtreePath, discoveryPattern)
        : discoveryPath(broker, discoveryPattern),
    );
  }

  function browseHref(prefix: string): string {
    return browsePath(broker, prefix, subtreePath, discoveryPattern);
  }

  function navigate(path: string) {
    if (location.hash === path) {
      applyRoute();
    } else {
      location.hash = path;
    }
  }

  function setExpanded(path: string, open: boolean) {
    browseState = browse.setExpanded(browseState, path, open);
  }

  function updateEditor(value: string) {
    browseState = browse.updateEditor(browseState, value);
  }

  function select(path: string) {
    browseState = browse.loadSelected(browseState, path);
  }

  function focusTreeItem(path: string) {
    requestAnimationFrame(() => {
      document
        .querySelector<HTMLElement>(`[data-tree-path="${CSS.escape(path)}"]`)
        ?.focus();
    });
  }

  function focusEditor() {
    requestAnimationFrame(() => {
      document.querySelector<HTMLTextAreaElement>("[data-leaf-editor]")?.focus();
    });
  }

  function activateBrowseTree(path: string, internal: boolean, open: boolean) {
    select(path);
    if (internal) {
      setExpanded(path, !open);
      return;
    }
    if (browse.selected(browseState)?.kind === "leaf") {
      focusEditor();
    }
  }

  function navigateBrowseTree(path: string, direction: NavDirection, step?: number): string {
    const next = browse.navigate(browseState, path, direction, step);
    browseState = next.state;
    return next.path;
  }

  function commitSettings({ settings: nextSettings, changed }: SettingsCommit) {
    const commit = browse.commitSettings(browseState, { settings: nextSettings, changed });
    browseState = commit.state;
    settingsRevision = commit.rev ?? settingsRevision;
    const at = performance.now();
    treeActivity = new Map([
      ...treeActivity,
      ...[...commit.cues].map((path) => [path, { at }] as const),
    ]);
    if (changed.size) {
      log("commit", `${changed.size} changed`);
    }
  }

  function resetBrowseState() {
    stopConnection?.();
    discoveryWatch?.close();
    prefixSession?.close();
    stopConnection = undefined;
    discoveryWatch = undefined;
    prefixSession = undefined;
    aliveManifest = undefined;
    settingsRevision = "";
    browseState = browse.emptyState();
    treeActivity = new Map();
  }

  function showDiscoveryIdle() {
    error = "";
    resetBrowseState();
    activePrefix = "";
    discoveredPrefixes = [];
    setStatus("Idle");
  }

  async function connectBackend(serial: number): Promise<MiniconfBackend | undefined> {
    backend?.close();
    const auth = username || password ? { username, password } : undefined;
    const next = await MiniconfBackend.connect(broker, auth);
    if (serial !== routeSerial) {
      next.close();
      return undefined;
    }
    backend = next;
    return next;
  }

  function loadSchema(nextSchema: Schema, root: string) {
    browseState = browse.loadSchema(browseState, nextSchema, root);
    subtreePath = browseState.root;
    syncUrl();
  }

  function log(event: string, detail: string) {
    eventLog.add(logOpen, event, detail);
  }

  function setStatus(next: string) {
    if (next === status) {
      return;
    }
    status = next;
    log("status", next);
  }

  function discover() {
    navigate(discoveryPath(broker, discoveryPattern));
  }

  async function startDiscovery(serial: number) {
    error = "";
    setStatus("Connecting");
    resetBrowseState();
    activePrefix = "";
    discoveredPrefixes = [];
    syncUrl();
    try {
      const nextBackend = await connectBackend(serial);
      if (!nextBackend) {
        return;
      }
      stopConnection = nextBackend.watchConnection((event) => {
        if (serial !== routeSerial) {
          return;
        }
        switch (event.state) {
          case "connected":
            setStatus("Broker reconnected; restoring discovery subscription");
            break;
          case "subscriptions-restored":
            setStatus("Watching discovery");
            break;
          case "reconnecting":
            setStatus("Broker reconnecting");
            break;
          case "offline":
          case "closed":
            setStatus("Broker disconnected");
            break;
          case "error":
            setStatus(event.transient ? "Broker reconnecting" : "Broker connection error");
            if (!event.transient) {
              error = event.error ?? "";
            }
            if (error && !event.transient) {
              log("error", error);
            }
            break;
        }
      });
      const watch = nextBackend.watchDiscovery(discoveryPattern, (next) => {
        if (serial !== routeSerial) {
          return;
        }
        discoveredPrefixes = next;
        setStatus(`${discoveredPrefixes.length} matching prefix${discoveredPrefixes.length === 1 ? "" : "es"}`);
      });
      discoveryWatch = watch;
      await watch.ready;
      if (serial !== routeSerial || discoveryWatch !== watch) {
        watch.close();
        return;
      }
      setStatus("Watching discovery");
    } catch (err) {
      if (serial !== routeSerial) {
        return;
      }
      error = err instanceof Error ? err.message : String(err);
      setStatus("Error");
      log("error", error);
    }
  }

  async function startBrowse(serial: number) {
    error = "";
    setStatus("Connecting");
    resetBrowseState();
    try {
      const nextBackend = await connectBackend(serial);
      if (!nextBackend) {
        return;
      }
      prefixSession = nextBackend.openPrefix(activePrefix, subtreePath, {
        error: (message) => {
          if (serial !== routeSerial) {
            return;
          }
          error = message;
          log("error", message);
        },
        alive: (next) => {
          if (serial !== routeSerial) {
            return;
          }
          aliveManifest = next;
        },
        response: (response) => {
          if (serial !== routeSerial) {
            return;
          }
          // ACK/NAK is request feedback only. Do not mirror values here; wait
          // for the authoritative /settings publication handled below.
          error = response.ok ? "" : `${response.code}: ${response.message}`;
          setStatus(response.ok
            ? `Set accepted for ${displayPath(response.path)}`
            : `Set rejected for ${displayPath(response.path)}`);
          log("response", `${response.code} ${displayPath(response.path)}`);
        },
        schema: (nextSchema, root) => {
          if (serial === routeSerial) {
            loadSchema(nextSchema, root);
          }
        },
        settings: (commit) => {
          if (serial === routeSerial) {
            commitSettings(commit);
          }
        },
        status: (next) => {
          if (serial !== routeSerial) {
            return;
          }
          setStatus(next);
        },
      });
      await prefixSession.open();
    } catch (err) {
      if (serial !== routeSerial) {
        return;
      }
      error = err instanceof Error ? err.message : String(err);
      setStatus("Error");
      log("error", error);
    }
  }

  async function submit() {
    if (!prefixSession || !selected || selected.kind !== "leaf") {
      return;
    }
    let value: unknown;
    try {
      value = browse.parseEditor(browseState);
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
      log("error", error);
      return;
    }
    error = "";
    try {
      const response = await prefixSession.set(selected.path, value);
      if (!response.ok) {
        error = `${response.code}: ${response.message}`;
      }
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
      setStatus("Set failed");
      log("error", error);
    }
  }

  function applyRoute() {
    const next = readRoute(location);
    // Route changes are the app-level cancellation boundary. Backend sessions
    // also serialize their own retained refreshes, but stale callbacks can still
    // arrive at this shell while navigation is in progress.
    const serial = ++routeSerial;
    broker = next.broker;
    discoveryPattern = next.discoveryPattern;
    activePrefix = next.activePrefix;
    subtreePath = next.subtreePath;
    if (next.page === "browse") {
      void startBrowse(serial);
    } else if (next.page === "discover") {
      void startDiscovery(serial);
    } else {
      showDiscoveryIdle();
    }
  }

  onMount(() => {
    addEventListener("hashchange", applyRoute);
    applyRoute();
    return () => {
      removeEventListener("hashchange", applyRoute);
      stopConnection?.();
      discoveryWatch?.close();
      prefixSession?.close();
      backend?.close();
    };
  });
</script>

<main>
  {#if mode === "discover"}
    <DiscoveryView
      bind:broker
      bind:discoveryPattern
      bind:username
      bind:password
      {discoveredPrefixes}
      {status}
      {error}
      bind:logOpen
      {logLines}
      {discover}
      {browseHref}
    />
  {:else}
    <BrowseView
      {broker}
      {activePrefix}
      discoverHref={discoveryPath(broker, discoveryPattern)}
      {subtreePath}
      {aliveManifest}
      {settingsRevision}
      {status}
      {error}
      treeNodes={browseState.tree.nodeViews}
      selectedPath={browseState.selectedPath}
      selected={selected}
      activity={treeActivity}
      expanded={browseState.expanded}
      editor={browseState.editor}
      bind:logOpen
      {logLines}
      treeRoot={browseState.root}
      treeActions={{
        activate: (node, internal, open) => activateBrowseTree(node.path, internal, open),
        key: (node, direction, step) => navigateBrowseTree(node.path, direction, step),
        open: setExpanded,
        select: (path) => select(path),
      }}
      {updateEditor}
      submit={() => void submit()}
      focusTree={() => focusTreeItem(browseState.selectedPath)}
      resetEditor={() => {
        browseState = browse.loadEditor(browseState);
      }}
    />
  {/if}
</main>
