<script lang="ts">
  let {
    label,
    value,
    heading = true,
  }: { label: string; value: unknown; heading?: boolean } = $props();
  let entries = $derived(
    value !== null &&
      typeof value === "object" &&
      !Array.isArray(value) &&
      Object.keys(value).length
      ? Object.entries(value)
      : [[null, value]],
  );
</script>

<section aria-label={label}>
  {#if heading}<h3>{label}</h3>{/if}
  <dl>
    {#each entries as [key, item]}
      <div>
        {#if key !== null}<dt>{key || '""'}</dt>{/if}
        <dd
          class:json={typeof item !== "string"}
          class:multiline={typeof item === "string" && item.includes("\n")}
        >
          {typeof item === "string" ? item : JSON.stringify(item, null, 2)}
        </dd>
      </div>
    {/each}
  </dl>
</section>

<style>
  section {
    margin-bottom: var(--space);
    min-width: 0;
  }
  h3 {
    font-size: var(--text);
    font-weight: 600;
    margin: 0 0 var(--space-tight);
  }
  dl {
    margin: 0;
  }
  dl > div {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 3fr);
    gap: 0 var(--space);
    margin-bottom: var(--space-tight);
  }
  dt {
    color: var(--muted);
    font-size: var(--text-small);
    overflow-wrap: anywhere;
  }
  dd {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .json {
    font-family: ui-monospace, monospace;
    font-size: var(--text-small);
  }
  .multiline,
  dd:only-child {
    grid-column: 1 / -1;
  }
</style>
