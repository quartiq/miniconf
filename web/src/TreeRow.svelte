<svelte:options runes={true} />

<script lang="ts">
  type Props = {
    path: string;
    label: string;
    selected: boolean;
    internal?: boolean;
    open?: boolean;
    depth?: number;
    level?: number;
    posinset?: number;
    setsize?: number;
    value?: string;
    href?: string;
    flashed?: boolean;
    title?: string;
    select: () => void;
    toggle: () => void;
    keydown: (event: KeyboardEvent) => void;
  };

  let {
    path,
    label,
    selected,
    internal = false,
    open = false,
    depth = 0,
    level = 1,
    posinset = 1,
    setsize = 1,
    value = "",
    href = undefined,
    flashed = false,
    title = "",
    select,
    toggle,
    keydown,
  }: Props = $props();

  const flashFrames = [
    { background: "var(--flash)" },
    { background: "var(--flash-end)" },
  ];

  function flash(node: HTMLElement, active: boolean) {
    let animation: Animation | undefined;
    const run = (next: boolean) => {
      if (!next) {
        return;
      }
      animation?.cancel();
      animation = node.animate(flashFrames, {
        duration: 1000,
        easing: "ease-out",
      });
    };
    run(active);
    return {
      update: run,
      destroy() {
        animation?.cancel();
      },
    };
  }

  function stopAndToggle(event: MouseEvent) {
    event.stopPropagation();
    toggle();
  }

  function stopAndSelect(event: MouseEvent) {
    event.stopPropagation();
    select();
  }
</script>

{#if href && !internal}
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
    use:flash={flashed}
    onclick={select}
    onkeydown={keydown}
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
    use:flash={flashed}
    onclick={select}
    onkeydown={keydown}
  >
    {#if internal}
      <button
        aria-label={open ? "Collapse" : "Expand"}
        class="toggle"
        tabindex="-1"
        type="button"
        onclick={stopAndToggle}
      >{open ? "▾" : "▸"}</button>
    {:else}
      <span aria-hidden="true" class="spacer"></span>
    {/if}
    {#if href}
      <a class="label" {href} tabindex="-1" onclick={stopAndSelect}>{label}</a>
    {:else}
      <span class="label">{label}</span>
    {/if}
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
    --flash-end: transparent;
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
    box-shadow: inset 2px 0 0 var(--selected-mark);
    --flash-end: var(--selected);
  }

  .label {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  a.label {
    color: inherit;
    text-decoration-thickness: 1px;
    text-underline-offset: 0.15em;
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

</style>
