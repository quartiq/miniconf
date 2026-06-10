<svelte:options runes={true} />

<script lang="ts">
  type Props = {
    status: string;
    error?: string;
    logLines?: string[];
    open?: boolean;
  };

  let {
    status,
    error = "",
    logLines = [],
    open = $bindable(false),
  }: Props = $props();
</script>

<details class="log" bind:open>
  <summary>
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
