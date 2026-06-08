<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { displayPath, type Schema } from "./lib/schema";
  import BrowseView from "./BrowseView.svelte";
  import DiscoveryView from "./DiscoveryView.svelte";
  import StatusLog from "./StatusLog.svelte";
  import {
    MiniconfBackend,
    type PrefixSession,
    type DiscoveredPrefix,
    type AliveManifest,
  } from "./lib/backend";
  import { loadAuth, saveAuth } from "./lib/auth-store";
  import { BrowseModel } from "./lib/browse-model";
  import { EventLog } from "./lib/event-log";
  import { FlashSet } from "./lib/flash-set";
  import { browsePath, discoveryPath, readRoute } from "./lib/routes";
  import { type SettingsCommit } from "./lib/settings-mirror";
  import { type NavDirection } from "./lib/tree-navigation";

  const route = readRoute(location);
  let broker = route.broker;
  let discoveryPattern = route.discoveryPattern;
  let activePrefix = route.activePrefix;
  let subtreePath = route.subtreePath;
  const initialAuth = loadAuth(broker);
  let username = initialAuth.username;
  let password = initialAuth.password;
  let authBroker = broker;

  let backend: MiniconfBackend | undefined;
  let prefixSession: PrefixSession | undefined;
  let discoveredPrefixes: DiscoveredPrefix[] = [];
  let aliveManifest: AliveManifest | undefined;
  let browse = new BrowseModel();
  let status = "Idle";
  let settingsRevision = "";
  let error = "";
  let logOpen = new URLSearchParams(location.search).get("log") === "1";
  let logLines: string[] = [];
  let stopConnection: (() => void) | undefined;
  let stopDiscovery: (() => void) | undefined;
  let routeSerial = 0;
  // Row flashes are UI cues for /settings echoes only. /set responses update
  // the status/log, but the retained/live settings mirror is authoritative.
  const treeFlash = new FlashSet((paths) => {
    browse.setFlashed(paths);
    browse = browse;
  });
  const eventLog = new EventLog(() => {
    logLines = eventLog.lines;
  });

  $: selected = browse.selected;
  $: mode = activePrefix ? "browse" : "discover";
  $: if (broker !== authBroker) {
    authBroker = broker;
    ({ username, password } = loadAuth(broker));
  }
  $: eventLog.clearHidden(logOpen);

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
    browse.setExpanded(path, open);
    browse = browse;
  }

  function updateEditor(value: string) {
    browse.updateEditor(value);
    browse = browse;
  }

  function select(path: string) {
    browse.loadSelected(path);
    browse = browse;
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
    if (browse.selected?.kind === "leaf") {
      focusEditor();
    }
  }

  function navigateBrowseTree(path: string, direction: NavDirection, step?: number) {
    const next = browse.navigate(path, direction, step);
    browse.loadSelected(next);
    browse = browse;
    focusTreeItem(next);
  }

  function commitSettings({ settings: nextSettings, changed }: SettingsCommit) {
    const commit = browse.commit({ settings: nextSettings, changed });
    settingsRevision = commit.rev ?? settingsRevision;
    browse = browse;
    treeFlash.add(commit.cues);
    if (changed.size) {
      log("commit", `${changed.size} changed`);
    }
  }

  function resetBrowseState() {
    stopConnection?.();
    stopDiscovery?.();
    prefixSession?.close();
    stopConnection = undefined;
    stopDiscovery = undefined;
    prefixSession = undefined;
    aliveManifest = undefined;
    settingsRevision = "";
    browse.reset();
    browse = browse;
    treeFlash.reset();
  }

  function showDiscoveryIdle() {
    error = "";
    resetBrowseState();
    activePrefix = "";
    discoveredPrefixes = [];
    setStatus("Idle");
  }

  function storeAuth() {
    saveAuth(broker, { username, password });
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
    browse.loadSchema(nextSchema, root);
    browse = browse;
    subtreePath = root;
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
    storeAuth();
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
      setStatus("Watching discovery");
      stopConnection = nextBackend.watchConnection((event) => {
        if (serial !== routeSerial) {
          return;
        }
        switch (event.state) {
          case "connected":
            setStatus("Broker reconnected; restoring discovery subscription");
            break;
          case "retained-replay-ready":
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
            setStatus("Broker connection error");
            error = event.error ?? "";
            if (error) {
              log("error", error);
            }
            break;
        }
      });
      stopDiscovery = nextBackend.watchDiscovery(discoveryPattern, (next) => {
        if (serial !== routeSerial) {
          return;
        }
        discoveredPrefixes = next;
        setStatus(`${discoveredPrefixes.length} matching prefix${discoveredPrefixes.length === 1 ? "" : "es"}`);
      });
    } catch (err) {
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
      value = browse.parseEditor();
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
  });

  onDestroy(() => {
    removeEventListener("hashchange", applyRoute);
    stopConnection?.();
    stopDiscovery?.();
    prefixSession?.close();
    treeFlash.reset();
    eventLog.dispose();
    backend?.close();
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
      {discover}
      {browseHref}
    />
    <StatusLog {status} {error} bind:open={logOpen} {logLines} />
  {:else}
    <BrowseView
      {activePrefix}
      discoverHref={discoveryPath(broker, discoveryPattern)}
      {subtreePath}
      {aliveManifest}
      {settingsRevision}
      {status}
      {error}
      rootNode={browse.rootNode}
      treeNodes={browse.treeNodes}
      selectedPath={browse.selectedPath}
      selected={browse.selected}
      flashed={browse.flashed}
      expanded={browse.expanded}
      editor={browse.editor}
      bind:logOpen
      {logLines}
      treeRoot={browse.root}
      treeActions={{
        activate: (node, internal, open) => activateBrowseTree(node.path, internal, open),
        key: (node, direction, step) => navigateBrowseTree(node.path, direction, step),
        open: setExpanded,
        select: (path) => select(path),
      }}
      {updateEditor}
      submit={() => void submit()}
      focusTree={() => focusTreeItem(browse.selectedPath)}
      resetEditor={() => {
        browse.loadEditor();
        browse = browse;
      }}
    />
  {/if}
</main>
