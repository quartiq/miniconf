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

  type Action = "Set" | "Prune";
  type ActionResult = { text: string; failed: boolean };

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

  let session = $state.raw<DiscoverySession | PrefixSession>();
  let deviceReady = $state(false);
  let discoveredPrefixes = $state<DiscoveredPrefix[]>([]);
  let aliveManifest = $state<AliveManifest | undefined>();
  let browseState = $state(browse.emptyState());
  let connection = $state<SessionStatus>({ state: "idle" });
  let actions = $state<{ pending: Set<Action>; result?: ActionResult }>({
    pending: new Set(),
  });
  let pruning = $state<PruningState>({
    count: 0,
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
      waiting: "Waiting for device",
      loading: "Loading schema",
      watching: activePrefix ? "Ready" : "Discovering devices",
      error: "Connection error",
      failed: "Connection failed",
      "device-error": "Device unavailable",
    }[connection.state],
  );
  let editorError = $state<{ path: string; text: string; message: string }>();
  let settingsRevision = $state("");
  let error = $state("");
  let browseStatus = $derived.by(() => {
    if (connection.state !== "watching") {
      return {
        text: [status, error, actions.result?.failed ? actions.result.text : ""]
          .filter(Boolean)
          .join(" · "),
        failed: !!error || !!actions.result?.failed,
      };
    }
    if (actions.result?.failed) return actions.result;
    if (actions.pending.has("Set")) return { text: "Setting…", failed: false };
    if (actions.pending.has("Prune"))
      return { text: "Pruning…", failed: false };
    return actions.result ?? { text: status, failed: false };
  });
  let logOpen = $state(new URLSearchParams(location.search).get("log") === "1");
  let logLines = $state<string[]>([]);
  const browseMemory = new Map<string, browse.BrowseMemory>();
  // Activity dots are UI cues for /settings echoes only. /set responses update
  // the status/log, but the retained/live settings mirror is authoritative.
  let treeActivity = $state.raw(new Map<string, TreeActivity>());
  const eventLog = new EventLog(() => {
    logLines = eventLog.lines;
  });

  let selected = $derived(browse.selected(browseState));
  let editor = $derived(browse.editor(browseState));
  let editorDirty = $derived(
    browseState.draft !== undefined && browseState.draft !== selected?.value,
  );
  let canSet = $derived(
    deviceReady && selected?.kind === "leaf" && !actions.pending.has("Set"),
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
    browseState = browse.loadSelected(browseState, path);
  }

  function focusTreeItem(path: string) {
    path = treeTabStop(
      path,
      visibleTreePaths(
        browseState.root,
        browseState.tree,
        browseState.expanded,
      ),
    );
    requestAnimationFrame(() => {
      const row = document.querySelector<HTMLElement>(
        `[data-tree-path="${CSS.escape(path)}"]`,
      );
      row?.focus({ preventScroll: true });
      row?.scrollIntoView({ block: "nearest", inline: "nearest" });
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
    // Invalidate captured route signals before closing can invoke callbacks.
    connectionAbort.abort();
    connectionAbort = new AbortController();
    session?.close();
    session = undefined;
    deviceReady = false;
    aliveManifest = undefined;
    settingsRevision = "";
    pruning = {
      count: 0,
      coverageWarning: "",
    };
    browseState = preserve
      ? browse.commitSettings(browseState, {
          settings: new Map(),
          touched: new Set(browseState.settings.keys()),
          activity: new Set(),
        }).state
      : browse.emptyState();
    actions = { pending: new Set() };
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
    const detail = "error" in next ? next.error : "";
    if (connection.state === next.state && error === detail) return;
    connection = next;
    error = detail;
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

  async function connectRoute() {
    const signal = connectionAbort.signal;
    error = "";
    setStatus({ state: "connecting" });
    discoveredPrefixes = [];
    syncUrl();
    const options = { auth: credentials, signal };
    try {
      let next: DiscoverySession | PrefixSession;
      if (activePrefix) {
        next = await PrefixSession.connect(
          broker,
          activePrefix,
          subtreePath,
          {
            alive: (next) => {
              if (signal.aborted) {
                return;
              }
              aliveManifest = next;
              if (!next) {
                settingsRevision = "";
                if (!actions.result?.failed) actions.result = undefined;
              }
            },
            schema: (nextSchema, root) => {
              if (!signal.aborted) {
                loadSchema(nextSchema, root);
              }
            },
            settings: (commit) => {
              if (!signal.aborted) {
                commitSettings(commit);
              }
            },
            pruning: (next) => {
              if (!signal.aborted) pruning = next;
            },
            status: (next, ready) => {
              if (signal.aborted) {
                return;
              }
              deviceReady = ready;
              setStatus(next);
            },
          },
          options,
        );
      } else {
        next = await DiscoverySession.connect(
          broker,
          discoveryPattern,
          {
            prefixes: (prefixes) => {
              if (!signal.aborted) discoveredPrefixes = prefixes;
            },
            status: (next) => {
              if (!signal.aborted) setStatus(next);
            },
          },
          options,
        );
      }
      if (signal.aborted) {
        next.close();
        return;
      }
      session = next;
    } catch (err) {
      if (signal.aborted) {
        return;
      }
      error = err instanceof Error ? err.message : String(err);
      setStatus({ state: "failed", error });
      log("error", error);
    }
  }

  async function perform(
    action: Action,
    operation: (session: PrefixSession) => Promise<ActionResult>,
    path?: string,
  ): Promise<boolean> {
    if (
      !(session instanceof PrefixSession) ||
      !deviceReady ||
      actions.pending.has(action)
    )
      return false;
    const current = session;
    const signal = connectionAbort.signal;
    actions = { pending: new Set([...actions.pending, action]) };
    let result: ActionResult;
    try {
      result = await operation(current);
    } catch (error) {
      result = {
        text: `${action}: ${error instanceof Error ? error.message : String(error)}`,
        failed: true,
      };
    }
    if (signal.aborted) return false;
    actions.pending = new Set(
      [...actions.pending].filter((item) => item !== action),
    );
    // An unrelated operation completing must not dismiss an unseen failure.
    if (!actions.result?.failed || result.failed) actions.result = result;
    log(
      action.toLowerCase(),
      path === undefined ? result.text : `${displayPath(path)}: ${result.text}`,
    );
    return !result.failed;
  }

  async function submit() {
    if (!canSet || !selected) return;
    const path = selected.path;
    actions.result = undefined;
    try {
      JSON.parse(editor);
    } catch (err) {
      editorError = {
        path,
        text: editor,
        message: `Invalid JSON: ${err instanceof Error ? err.message : String(err)}`,
      };
      return;
    }
    const succeeded = await perform(
      "Set",
      async (session) => {
        const response = await session.set(path, editor);
        return {
          failed: !response.ok,
          text: response.ok
            ? "Set succeeded"
            : response.kind === "publish"
              ? `Set: value may have changed — publication failed. ${response.message}`
              : `Set failed: ${response.message || response.code}`,
        };
      },
      path,
    );
    if (succeeded && browseState.selectedPath === path) {
      browseState = browse.loadEditor(browseState);
      editorError = undefined;
    }
  }

  function prune() {
    void perform("Prune", async (session) => {
      const result = await session.prune();
      return {
        failed: result.error !== undefined,
        text:
          result.error === undefined
            ? `Cleared ${result.cleared}`
            : `Cleared ${result.cleared}; pruning interrupted, remaining outcome unknown. ${result.error}`,
      };
    });
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
    if (next.page === "landing") {
      showDiscoveryIdle();
    } else {
      void connectRoute();
    }
  }

  onMount(() => {
    addEventListener("hashchange", applyRoute);
    applyRoute();
    return () => {
      removeEventListener("hashchange", applyRoute);
      connectionAbort.abort();
      session?.close();
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
      status={browseStatus}
      retryable={connection.state === "failed" ||
        connection.state === "device-error"}
      treeNodes={browseState.tree}
      selectedPath={browseState.selectedPath}
      {selected}
      activity={treeActivity}
      expanded={browseState.expanded}
      {editor}
      {editorDirty}
      editorError={editorError?.path === browseState.selectedPath &&
      editorError.text === editor
        ? editorError.message
        : ""}
      {canSet}
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
      canPrune={deviceReady && !actions.pending.has("Prune")}
      {prune}
    />
  {/if}
</main>
