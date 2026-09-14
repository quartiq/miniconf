<svelte:options runes={true} />

<script lang="ts">
  import { onMount } from "svelte";
  import { displayPath, type Schema } from "./lib/schema";
  import BrowseView from "./BrowseView.svelte";
  import DiscoveryView from "./DiscoveryView.svelte";
  import {
    DiscoverySession,
    PrefixSession,
    type DiscoveredPrefix,
    type AliveManifest,
    type SessionStatus,
    type PruningState,
  } from "./lib/backend";
  import * as browse from "./lib/browse-model";
  import { EventLog } from "./lib/event-log";
  import { browsePath, discoveryPath, readRoute } from "./lib/routes";
  import { type SettingsCommit } from "./lib/settings-mirror";
  import {
    treeTabStop,
    visibleTreePaths,
    type NavDirection,
  } from "./lib/tree-navigation";
  import { type TreeActivity } from "./lib/tree-view";

  const route = readRoute(location);
  let broker = $state(route.broker);
  let discoveryPattern = $state(route.discoveryPattern);
  let activePrefix = $state(route.activePrefix);
  let subtreePath = $state(route.subtreePath);
  let formBroker = $state(route.broker);
  let formPattern = $state(route.discoveryPattern);
  let credentials = $state<{
    broker: string;
    username: string;
    password: string;
  }>();
  let connectionAbort = new AbortController();
  let username = $state("");
  let password = $state("");

  let discoverySession: DiscoverySession | undefined;
  let prefixSession = $state.raw<PrefixSession>();
  let deviceReady = $state(false);
  let discoveredPrefixes = $state<DiscoveredPrefix[]>([]);
  let aliveManifest = $state<AliveManifest | undefined>();
  let browseState = $state(browse.emptyState());
  let connection = $state<SessionStatus>({ state: "idle" });
  let pruning = $state<PruningState>({
    count: 0,
    pending: false,
    message: "",
    coverageWarning: "",
  });
  let status = $derived(
    {
      idle: "Not connected",
      connecting: "Connecting",
      connected: "Connected",
      restoring: "Restoring subscriptions",
      reconnecting: "Reconnecting",
      offline: "Disconnected — last observed values",
      waiting: "Waiting for device announcement",
      loading: "Loading schema",
      watching: activePrefix
        ? pruning.message || "Ready"
        : "Watching discovery",
      error: "Connection error",
      failed: "Connection failed",
      "device-error": "Device unavailable",
    }[connection.state],
  );
  let request = $state<{ path: string; pending: boolean; message: string }>();
  let editorError = $state<{ path: string; text: string; message: string }>();
  let settingsRevision = $state("");
  let error = $state("");
  let logOpen = $state(new URLSearchParams(location.search).get("log") === "1");
  let logLines = $state<string[]>([]);
  let routeSerial = $state(0);
  const browseMemory = new Map<string, browse.BrowseMemory>();
  // Activity dots are UI cues for /settings echoes only. /set responses update
  // the status/log, but the retained/live settings mirror is authoritative.
  let treeActivity = $state.raw(new Map<string, TreeActivity>());
  const eventLog = new EventLog(() => {
    logLines = eventLog.lines;
  });

  let selected = $derived(browse.selected(browseState));
  let editorDirty = $derived(
    browseState.editor !== (browseState.editorBaseline ?? ""),
  );
  let canSet = $derived(
    deviceReady && selected?.kind === "leaf" && !request?.pending,
  );
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
    if (path !== browseState.selectedPath && !request?.pending)
      request = undefined;
    browseState = browse.loadSelected(browseState, path);
  }

  function focusTreeItem(path: string) {
    path = treeTabStop(
      path,
      visibleTreePaths(
        browseState.root,
        browseState.tree.flatNodes,
        browseState.expanded,
      ),
    );
    requestAnimationFrame(() => {
      document
        .querySelector<HTMLElement>(`[data-tree-path="${CSS.escape(path)}"]`)
        ?.focus();
    });
  }

  function focusEditor() {
    requestAnimationFrame(() => {
      document
        .querySelector<HTMLTextAreaElement>("[data-leaf-editor]")
        ?.focus();
    });
  }

  function activateBrowseTree(path: string, internal: boolean, open: boolean) {
    if (internal) {
      setExpanded(path, !open);
      return;
    }
    select(path);
    if (browse.selected(browseState)?.kind === "leaf") {
      focusEditor();
    }
  }

  function navigateBrowseTree(
    path: string,
    direction: NavDirection,
    step?: number,
  ): string {
    const next = browse.navigate(browseState, path, direction, step);
    browseState = next.state;
    return next.path;
  }

  function commitSettings({
    settings: nextSettings,
    touched,
    activity,
    rev,
  }: SettingsCommit) {
    const commit = browse.commitSettings(browseState, {
      settings: nextSettings,
      touched,
      activity,
      rev,
    });
    browseState = commit.state;
    settingsRevision = commit.rev ?? settingsRevision;
    const at = Date.now();
    treeActivity = new Map([
      ...treeActivity,
      ...[...commit.cues].map((path) => [path, { at }] as const),
    ]);
    if (touched.size) {
      log("commit", `${touched.size} touched`);
    }
  }

  function resetBrowseState(preserve = false) {
    connectionAbort.abort();
    connectionAbort = new AbortController();
    discoverySession?.close();
    prefixSession?.close();
    discoverySession = undefined;
    prefixSession = undefined;
    deviceReady = false;
    aliveManifest = undefined;
    settingsRevision = "";
    pruning = { count: 0, pending: false, message: "", coverageWarning: "" };
    browseState = preserve
      ? browse.commitSettings(browseState, {
          settings: new Map(),
          touched: new Set(browseState.settings.keys()),
          activity: new Set(),
        }).state
      : browse.emptyState();
    request = undefined;
    editorError = undefined;
    treeActivity = new Map();
  }

  function showDiscoveryIdle() {
    error = "";
    activePrefix = "";
    discoveredPrefixes = [];
    setStatus({ state: "idle" });
  }

  function loadSchema(nextSchema: Schema, root: string) {
    const memory = browseState.schema
      ? browseState
      : browseMemory.get(JSON.stringify([broker, activePrefix, root]));
    browseState = browse.loadSchema(browseState, nextSchema, root, memory);
    subtreePath = browseState.root;
    syncUrl();
  }

  function log(event: string, detail: string) {
    eventLog.add(logOpen, event, detail);
  }

  function setStatus(next: SessionStatus) {
    if (
      connection.state === next.state &&
      (!("error" in next) ||
        ("error" in connection && connection.error === next.error))
    )
      return;
    connection = next;
    error = "error" in next ? next.error : "";
    log("status", next.state);
  }

  function discover() {
    try {
      const path = discoveryPath(formBroker.trim(), formPattern);
      credentials = {
        broker: readRoute({ hash: path }).broker,
        username,
        password,
      };
      navigate(path);
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    }
  }

  async function startDiscovery(serial: number) {
    error = "";
    setStatus({ state: "connecting" });
    activePrefix = "";
    discoveredPrefixes = [];
    syncUrl();
    try {
      const next = await DiscoverySession.connect(
        broker,
        discoveryPattern,
        {
          prefixes: (prefixes) => {
            if (serial !== routeSerial) return;
            discoveredPrefixes = prefixes;
          },
          status: (nextStatus) => {
            if (serial === routeSerial) setStatus(nextStatus);
          },
        },
        { auth: credentials, signal: connectionAbort.signal },
      );
      if (serial !== routeSerial) {
        next.close();
        return;
      }
      discoverySession = next;
    } catch (err) {
      if (serial !== routeSerial) {
        return;
      }
      error = err instanceof Error ? err.message : String(err);
      setStatus({ state: "failed", error });
      log("error", error);
    }
  }

  async function startBrowse(serial: number) {
    error = "";
    setStatus({ state: "connecting" });
    try {
      const next = await PrefixSession.connect(
        broker,
        activePrefix,
        subtreePath,
        {
          alive: (next) => {
            if (serial !== routeSerial) {
              return;
            }
            aliveManifest = next;
            if (!next) settingsRevision = "";
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
          pruning: (next) => {
            if (serial !== routeSerial) return;
            if (next.message && next.message !== pruning.message)
              log("prune", next.message);
            pruning = next;
          },
          status: (next, ready) => {
            if (serial !== routeSerial) {
              return;
            }
            deviceReady = ready;
            setStatus(next);
          },
        },
        { auth: credentials, signal: connectionAbort.signal },
      );
      if (serial !== routeSerial) {
        next.close();
        return;
      }
      prefixSession = next;
    } catch (err) {
      if (serial !== routeSerial) {
        return;
      }
      error = err instanceof Error ? err.message : String(err);
      setStatus({ state: "failed", error });
      log("error", error);
    }
  }

  async function submit() {
    if (!canSet || !prefixSession || !selected) return;
    const current = prefixSession;
    const serial = routeSerial;
    const path = selected.path;
    try {
      JSON.parse(browseState.editor);
    } catch (err) {
      editorError = {
        path,
        text: browseState.editor,
        message: `Invalid JSON: ${err instanceof Error ? err.message : String(err)}`,
      };
      return;
    }
    request = { path, pending: true, message: "Setting…" };
    try {
      const response = await current.set(path, browseState.editor);
      if (serial !== routeSerial || current !== prefixSession) return;
      request = {
        path,
        pending: false,
        message: response.ok
          ? "Last Set: succeeded"
          : response.kind === "publish"
            ? `Last Set: value may have changed — publication failed. ${response.message}`
            : `Last Set: failed — ${response.message || response.code}`,
      };
    } catch (err) {
      if (serial !== routeSerial || current !== prefixSession) return;
      request = {
        path,
        pending: false,
        message: `Last Set: ${err instanceof Error ? err.message : String(err)}`,
      };
    }
    if (request) log("request", `${displayPath(path)}: ${request.message}`);
  }

  function applyRoute() {
    const next = readRoute(location);
    if (browseState.schema && activePrefix) {
      browseMemory.set(
        JSON.stringify([broker, activePrefix, browseState.root]),
        {
          expanded: new Set(browseState.expanded),
          selectedPath: browseState.selectedPath,
          userClosed: new Set(browseState.userClosed),
        },
      );
    }
    // Route changes cancel the old session; the serial also rejects callbacks
    // from an initial connection that completed after navigation.
    const serial = ++routeSerial;
    const preserve =
      next.page === "browse" &&
      next.broker === broker &&
      next.activePrefix === activePrefix &&
      next.subtreePath === subtreePath;
    resetBrowseState(preserve);
    if (credentials?.broker !== next.broker) {
      credentials = undefined;
      username = "";
      password = "";
    }
    formBroker = next.broker;
    formPattern = next.discoveryPattern;
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
      routeSerial += 1;
      connectionAbort.abort();
      discoverySession?.close();
      prefixSession?.close();
    };
  });
</script>

<main>
  {#if mode === "discover"}
    <DiscoveryView
      bind:broker={formBroker}
      bind:discoveryPattern={formPattern}
      bind:username
      bind:password
      {discoveredPrefixes}
      watching={connection.state === "watching"}
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
      retryable={connection.state === "failed" ||
        connection.state === "device-error"}
      treeNodes={browseState.tree.nodeViews}
      selectedPath={browseState.selectedPath}
      {selected}
      activity={treeActivity}
      expanded={browseState.expanded}
      editor={browseState.editor}
      {editorDirty}
      editorError={editorError?.path === browseState.selectedPath &&
      editorError.text === browseState.editor
        ? editorError.message
        : ""}
      {canSet}
      requestMessage={request?.path === browseState.selectedPath
        ? request.message
        : ""}
      bind:logOpen
      {logLines}
      treeRoot={browseState.root}
      treeActions={{
        activate: (node, internal, open) =>
          activateBrowseTree(node.path, internal, open),
        key: (node, direction, step) =>
          navigateBrowseTree(node.path, direction, step),
        open: setExpanded,
        select: (path) => select(path),
      }}
      {updateEditor}
      submit={() => void submit()}
      focusTree={() => focusTreeItem(browseState.selectedPath)}
      resetEditor={() => {
        browseState = browse.loadEditor(browseState);
      }}
      retry={applyRoute}
      {pruning}
      canPrune={deviceReady && !pruning.pending}
      prune={() => void prefixSession?.prune()}
    />
  {/if}
</main>
