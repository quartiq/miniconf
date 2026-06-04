<script lang="ts">
  export let status: string;
  export let error = "";
  export let logLines: string[] = [];
  export let open = false;
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
