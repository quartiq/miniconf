<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { RetainedPruner, type PruneState } from "./lib/prune";
  import type { PrefixSession } from "./lib/backend";
  import type { MqttAuth } from "./lib/mqtt-session";

  let {
    broker,
    prefix,
    session,
    enabled,
    auth,
  }: {
    broker: string;
    prefix: string;
    session: PrefixSession | undefined;
    enabled: boolean;
    auth: Partial<MqttAuth> | undefined;
  } = $props();
  let review = $state<PruneState>();
  let busy = $state(false);
  let open = $state(false);
  let pruner: RetainedPruner | undefined;
  let controller: AbortController | undefined;

  function close() {
    controller?.abort();
    pruner?.close();
    controller = undefined;
    pruner = undefined;
  }
  $effect(() => {
    const observe = open && enabled;
    untrack(() => {
      if (observe && !controller) void scan();
      else if (!open && !busy) close();
    });
  });
  onDestroy(close);

  async function scan() {
    if (!enabled || !session) return;
    close();
    const current = new AbortController();
    controller = current;
    busy = true;
    review = { topics: [], ready: false, message: "Checking retained topics…" };
    try {
      const next = await RetainedPruner.connect(
        broker,
        prefix,
        session.pruningContext,
        (state) => {
          if (controller === current) review = state;
        },
        { auth, signal: current.signal },
      );
      if (controller !== current) next.close();
      else pruner = next;
    } catch (error) {
      if (controller === current)
        review = { topics: [], ready: false, message: String(error) };
    } finally {
      if (controller === current) {
        busy = false;
        if (!open) close();
      }
    }
  }
  async function clear() {
    if (!pruner || !review?.ready || busy) return;
    busy = true;
    try {
      await pruner.clear([...review.topics]);
    } catch {
      /* The review reports partial completion and requires a new scan. */
    } finally {
      busy = false;
      if (!open) close();
    }
  }
</script>

<details class="panel pruning" bind:open>
  <summary>Prune stale retained topics</summary>
  <p class="meta">
    Entire device prefix: clear broker-retained /set and /settings topics
    outside its schema, and retained /response messages. Valid settings are
    kept.
  </p>
  {#if review}
    <p role="status">{review.message}</p>
    {#if review.topics.length || review.ready}
      <pre>{review.topics.join("\n") || "No stale topics observed."}</pre>
    {/if}
    {#if !review.ready && !busy}
      <button type="button" disabled={!enabled} onclick={scan}>Retry</button>
    {/if}
    {#if review.topics.length}
      <button type="button" disabled={!review.ready || busy} onclick={clear}
        >Clear {review.topics.length} retained topics</button
      >
    {/if}
  {:else}
    <p role="status">Waiting for the device schema.</p>
  {/if}
</details>

<style>
  pre {
    max-height: calc(6 * var(--line));
    overflow: auto;
  }
  summary {
    cursor: pointer;
  }
  p {
    margin-block: var(--space-tight);
  }
</style>
