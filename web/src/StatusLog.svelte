<svelte:options runes={true} />

<script lang="ts">
  type Props = {
    status: string;
    error?: string;
    logLines?: string[];
    open?: boolean;
    live?: boolean;
  };

  let {
    status,
    error = "",
    logLines = [],
    open = $bindable(false),
    live = false,
  }: Props = $props();
</script>

<details class="log" bind:open>
  <summary aria-live={live ? "polite" : undefined}>
    <span aria-hidden="true" class="caret">{open ? "▾" : "▸"}</span>
    <span>{status}</span>
    {#if error}
      <strong>{error}</strong>
    {/if}
  </summary>
  <div class="log-body">
    {#if logLines.length}
      <pre>{logLines.join("\n")}</pre>
    {:else}
      <p>No log entries.</p>
    {/if}
  </div>
</details>
