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

  let discoverySession: DiscoverySession | undefined;
  let prefixSession: PrefixSession | undefined;
  let discoveredPrefixes = $state<DiscoveredPrefix[]>([]);
  let aliveManifest = $state<AliveManifest | undefined>();
  let browseState = $state(browse.emptyState());
  let status = $state("Idle");
  let settingsRevision = $state("");
  let error = $state("");
  let logOpen = $state(new URLSearchParams(location.search).get("log") === "1");
  let logLines = $state<string[]>([]);
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

  function commitSettings({ settings: nextSettings, changed, activity, rev }: SettingsCommit) {
    const commit = browse.commitSettings(browseState, { settings: nextSettings, changed, activity, rev });
    browseState = commit.state;
    settingsRevision = commit.rev ?? settingsRevision;
    const at = Date.now();
    treeActivity = new Map([
      ...treeActivity,
      ...[...commit.cues].map((path) => [path, { at }] as const),
    ]);
    if (changed.size) {
      log("commit", `${changed.size} changed`);
    }
  }

  function resetBrowseState() {
    discoverySession?.close();
    prefixSession?.close();
    discoverySession = undefined;
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

  function loadSchema(nextSchema: Schema, root: string) {
    browseState = browse.loadSchema(browseState, nextSchema, root);
    subtreePath = browseState.root;
    syncUrl();
  }

  function log(event: string, detail: string) {
    eventLog.add(logOpen, event, detail);
  }

  function setStatus(next: string, nextError = "") {
    error = nextError;
    if (next === status) return;
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
      const next = await DiscoverySession.connect(
        broker,
        discoveryPattern,
        {
          prefixes: (prefixes) => {
            if (serial !== routeSerial) return;
            discoveredPrefixes = prefixes;
            setStatus(`${prefixes.length} matching prefix${prefixes.length === 1 ? "" : "es"}`);
          },
          error: (message) => {
            if (serial !== routeSerial) return;
            error = message;
            log("error", message);
          },
          status: (nextStatus) => {
            if (serial === routeSerial) setStatus(nextStatus);
          },
        },
        username || password ? { username, password } : undefined,
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
      setStatus("Error", error);
      log("error", error);
    }
  }

  async function startBrowse(serial: number) {
    error = "";
    setStatus("Connecting");
    resetBrowseState();
    try {
      const next = await PrefixSession.connect(broker, activePrefix, subtreePath, {
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
          const responseError = response.ok ? "" : `${response.code}: ${response.message}`;
          setStatus(
            response.ok
              ? `Set accepted for ${displayPath(response.path)}`
              : `Set rejected for ${displayPath(response.path)}`,
            responseError,
          );
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
      }, username || password ? { username, password } : undefined);
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
      setStatus("Error", error);
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
      await prefixSession.set(selected.path, value);
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
      setStatus("Set failed", error);
      log("error", error);
    }
  }

  function applyRoute() {
    const next = readRoute(location);
    // Route changes cancel the old session; the serial also rejects callbacks
    // from an initial connection that completed after navigation.
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
      discoverySession?.close();
      prefixSession?.close();
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
      editorDirty={browseState.editorDirty}
      editorStale={browseState.editorStale}
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
