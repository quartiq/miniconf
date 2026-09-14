<script lang="ts">
  import { onDestroy } from "svelte";
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

  function cancel() {
    controller?.abort();
    pruner?.close();
    controller = undefined;
    pruner = undefined;
    review = undefined;
    busy = false;
  }
  $effect(() => {
    if (!open || !enabled) cancel();
  });
  onDestroy(cancel);

  async function scan() {
    if (!enabled || !session) return;
    cancel();
    const current = new AbortController();
    controller = current;
    busy = true;
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
      if (controller === current) busy = false;
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
    }
  }
</script>

<details class="panel pruning" bind:open>
  <summary>Prune stale retained topics</summary>
  <p class="meta">
    Clear broker-retained /set and /settings topics outside this device’s
    schema, and retained /response messages. Valid settings are kept.
  </p>
  <button type="button" disabled={!enabled || busy} onclick={scan}
    >Review stale topics</button
  >
  {#if review}
    <p role="status">{review.message}</p>
    <pre>{review.topics.join("\n") || "No stale topics observed."}</pre>
    <button
      type="button"
      disabled={!review.ready || !review.topics.length || busy}
      onclick={clear}>Clear {review.topics.length} retained topics</button
    >
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
