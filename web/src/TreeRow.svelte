<script lang="ts">
  export let path: string;
  export let label: string;
  export let selected: boolean;
  export let internal = false;
  export let open = false;
  export let depth = 0;
  export let level = 1;
  export let posinset = 1;
  export let setsize = 1;
  export let value = "";
  export let href: string | undefined = undefined;
  export let flashed = false;
  export let title = "";
  export let select: () => void;
  export let toggle: () => void;
  export let keydown: (event: KeyboardEvent) => void;
</script>

{#if href}
  <a
    aria-level={level}
    aria-posinset={posinset}
    aria-selected={selected}
    aria-setsize={setsize}
    class:flash={flashed}
    class:selected
    data-tree-path={path}
    {href}
    role="treeitem"
    style:padding-left={`${depth}rem`}
    tabindex={selected ? 0 : -1}
    {title}
    on:click={select}
    on:keydown={keydown}
  >
    <span aria-hidden="true" class="spacer"></span>
    <span class="label">{label}</span>
    {#if value}
      <span class="separator"> = </span>
      <span class="value">{value}</span>
    {/if}
  </a>
{:else}
  <div
    aria-expanded={internal ? open : undefined}
    aria-level={level}
    aria-posinset={posinset}
    aria-selected={selected}
    aria-setsize={setsize}
    class:flash={flashed}
    class:selected
    data-tree-path={path}
    role="treeitem"
    style:padding-left={`${depth}rem`}
    tabindex={selected ? 0 : -1}
    {title}
    on:click={select}
    on:keydown={keydown}
  >
    {#if internal}
      <button
        aria-label={open ? "Collapse" : "Expand"}
        class="toggle"
        tabindex="-1"
        type="button"
        on:click|stopPropagation={toggle}
      >{open ? "▾" : "▸"}</button>
    {:else}
      <span aria-hidden="true" class="spacer"></span>
    {/if}
    <span class="label">{label}</span>
    {#if value}
      <span class="separator"> = </span>
      <span class="value">{value}</span>
    {/if}
  </div>
{/if}

<style>
  [role="treeitem"] {
    align-items: baseline;
    color: inherit;
    display: flex;
    gap: 0;
    line-height: var(--line);
    max-width: 100%;
    min-height: var(--line);
    min-width: 0;
    overflow: hidden;
    padding-right: var(--space-tight);
    border-radius: var(--radius);
    text-align: left;
    text-decoration: none;
    width: 100%;
  }

  div[role="treeitem"] {
    cursor: default;
  }

  [role="treeitem"]:focus-visible {
    outline: 1px solid var(--focus);
    outline-offset: -1px;
  }

  button.toggle,
  .spacer {
    appearance: none;
    background: transparent;
    border: 0;
    color: inherit;
    display: inline-block;
    flex: 0 0 var(--caret);
    font: inherit;
    line-height: inherit;
    margin: 0;
    padding: 0;
    text-align: left;
    width: var(--caret);
  }

  button.toggle {
    cursor: pointer;
  }

  .selected {
    background: var(--selected);
  }

  .label {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .value {
    flex: 1 1 auto;
    min-width: 0;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .separator {
    flex: none;
    color: var(--muted);
    white-space: pre;
  }

  .flash {
    animation: flash 1s;
  }

  @keyframes flash {
    0% {
      background: var(--flash);
    }

    100% {
      background: transparent;
    }
  }
</style>
