<svelte:options runes={true} />

<script lang="ts">
  import { onMount } from "svelte";
  import type { Schema } from "./lib/schema";
  import BrowseView from "./BrowseView.svelte";
  import DiscoveryView from "./DiscoveryView.svelte";
  import { PrefixSession, type DiscoveredPrefix } from "./lib/backend";
  import {
    BrowseModel,
    rememberRoute,
    type BrowseMemory,
  } from "./lib/browse-model.svelte";
  import { EventLog } from "./lib/event-log";
  import { browsePath, discoveryPath, readRoute } from "./lib/routes";
  import { restoreAuth } from "./lib/session-auth";
  import { Connection } from "./lib/connection.svelte";
  import type { MqttAuth } from "./lib/mqtt-session";

  const buildCommit = __BUILD_COMMIT__;
  const buildUrl = /^[0-9a-f]{40}$/i.test(buildCommit)
    ? `https://github.com/quartiq/miniconf/commit/${buildCommit}`
    : undefined;

  const initialRoute = readRoute(location);
  const initialAuth = restoreAuth(initialRoute.broker);
  let route = $state(initialRoute);
  const connection = new Connection({
    alive: (next) => model.observeAlive(next),
    schema: loadSchema,
    settings: (commit) => model.commitSettings(commit),
    pruning: (next) => {
      model.pruning = next;
    },
    prefixes: (next) => {
      discoveredPrefixes = next;
    },
    status: (next) => {
      log("status", next.state);
    },
  });
  connection.credentials = initialAuth
    ? { broker: initialRoute.broker, ...initialAuth }
    : undefined;
  let discoveredPrefixes = $state<DiscoveredPrefix[]>([]);
  const model = new BrowseModel(
    () =>
      connection.ready && connection.session instanceof PrefixSession
        ? { session: connection.session, signal: connection.signal }
        : undefined,
    log,
  );
  let connectionPrompt = $derived(
    connection.status.state === "credentials" ||
      (connection.status.state === "failed" && !connection.session),
  );
  let status = $derived(
    {
      idle: "Not connected",
      credentials: "Enter credentials to reconnect",
      connecting: "Connecting",
      connected: "Connected",
      restoring: "Restoring subscriptions",
      reconnecting: "Reconnecting",
      offline: route.activePrefix
        ? "Disconnected — last observed values"
        : "Disconnected",
      waiting: "Waiting for device",
      loading: "Loading schema",
      watching: route.activePrefix ? "Ready" : "Discovering devices",
      error: "Connection error",
      failed: "Connection failed",
      "device-error": "Device unavailable",
    }[connection.status.state],
  );
  let error = $derived(
    "error" in connection.status ? connection.status.error : "",
  );
  let browseStatus = $derived.by(() => {
    if (connection.status.state !== "watching") {
      return {
        text: [status, error, model.result?.failed ? model.result.text : ""]
          .filter(Boolean)
          .join(" · "),
        failed: !!error || !!model.result?.failed,
      };
    }
    if (model.result?.failed) return model.result;
    if (model.pending.has("Set")) return { text: "Setting…", failed: false };
    if (model.pending.has("Prune")) return { text: "Pruning…", failed: false };
    return (
      model.result ?? {
        text: [status, connection.notice].filter(Boolean).join(" · "),
        failed: false,
      }
    );
  });
  let logOpen = $state(new URLSearchParams(location.search).get("log") === "1");
  let logLines = $state<string[]>([]);
  const browseMemory = new Map<string, BrowseMemory>();
  const eventLog = new EventLog(() => {
    logLines = eventLog.lines;
  });

  let mode = $derived(
    route.activePrefix && !connectionPrompt ? "browse" : "discover",
  );

  $effect(() => {
    eventLog.clearHidden(logOpen);
  });

  function syncUrl() {
    history.replaceState(
      {
        credentialBroker:
          connection.credentials?.username || connection.credentials?.password
            ? route.broker
            : undefined,
      },
      "",
      route.activePrefix
        ? browsePath(
            route.broker,
            route.activePrefix,
            route.subtreePath,
            route.discoveryFilter,
          )
        : discoveryPath(route.broker, route.discoveryFilter),
    );
  }

  function browseHref(prefix: string): string {
    return browsePath(
      route.broker,
      prefix,
      route.subtreePath,
      route.discoveryFilter,
    );
  }

  function navigate(path: string) {
    if (location.hash === path) {
      applyRoute();
    } else {
      location.hash = path;
    }
  }

  function loadSchema(nextSchema: Schema, root: string) {
    const memory = model.state.schema
      ? model.state
      : browseMemory.get(
          JSON.stringify([route.broker, route.activePrefix, root]),
        );
    model.loadSchema(nextSchema, root, memory);
    route.subtreePath = model.state.root;
    syncUrl();
  }

  function log(event: string, detail: string) {
    eventLog.add(logOpen, event, detail);
  }

  function connect(brokerDraft: string, filter: string, auth: MqttAuth) {
    let path = discoveryPath(brokerDraft.trim(), filter);
    const nextBroker = readRoute({ hash: path }).broker;
    if (connectionPrompt && route.activePrefix && nextBroker === route.broker)
      path = browsePath(
        nextBroker,
        route.activePrefix,
        route.subtreePath,
        filter,
      );
    connection.credentials = { broker: nextBroker, ...auth };
    navigate(path);
  }

  function applyRoute() {
    const next = readRoute(location);
    if (model.state.schema && route.activePrefix) {
      rememberRoute(
        browseMemory,
        JSON.stringify([route.broker, route.activePrefix, model.state.root]),
        model.state,
      );
    }
    const preserve =
      next.page === "browse" &&
      next.broker === route.broker &&
      next.activePrefix === route.activePrefix &&
      next.subtreePath === route.subtreePath;
    model.reset(preserve);
    route = next;
    discoveredPrefixes = [];
    void connection.open(
      next,
      history.state?.credentialBroker === route.broker,
    );
    if (next.page !== "landing") syncUrl();
  }

  onMount(() => {
    addEventListener("hashchange", applyRoute);
    applyRoute();
    return () => {
      removeEventListener("hashchange", applyRoute);
      connection.close();
    };
  });
</script>

<main>
  {#if mode === "discover"}
    <DiscoveryView
      broker={route.broker}
      discoveryFilter={route.discoveryFilter}
      auth={connection.credentials}
      {discoveredPrefixes}
      watching={connection.status.state === "watching"}
      status={[status, connection.notice].filter(Boolean).join(" · ")}
      {error}
      bind:logOpen
      {logLines}
      {connect}
      submitLabel={connectionPrompt && route.activePrefix
        ? "Reconnect"
        : "Discover"}
      {browseHref}
    />
  {:else}
    <BrowseView
      broker={route.broker}
      activePrefix={route.activePrefix}
      discoverHref={discoveryPath(route.broker, route.discoveryFilter)}
      subtreePath={route.subtreePath}
      {model}
      status={browseStatus}
      retryable={connection.status.state === "failed" ||
        connection.status.state === "device-error"}
      bind:logOpen
      {logLines}
      retry={applyRoute}
    />
  {/if}
  <footer>
    {#if buildUrl}
      <a href={buildUrl} target="_blank" rel="noreferrer"
        >build {buildCommit.slice(0, 8)}</a
      >
    {:else}
      local build
    {/if}
  </footer>
</main>

<style>
  footer {
    color: var(--muted);
    font-size: var(--text-small);
    margin-top: var(--space);
  }
</style>
