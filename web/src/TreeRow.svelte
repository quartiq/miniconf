<svelte:options runes={true} />

<script lang="ts">
  import type { TreeActivity } from "./lib/tree-view";

  type Props = {
    path: string;
    label: string;
    summary?: string;
    selected: boolean;
    tabbable: boolean;
    internal?: boolean;
    open?: boolean;
    depth?: number;
    level?: number;
    posinset?: number;
    setsize?: number;
    value?: string;
    href?: string;
    activity?: TreeActivity;
    showActivity?: boolean;
    title?: string;
    select: () => void;
    toggle: () => void;
    keydown: (event: KeyboardEvent) => void;
  };

  let {
    path,
    label,
    summary = "",
    selected,
    tabbable,
    internal = false,
    open = false,
    depth = 0,
    level = 1,
    posinset = 1,
    setsize = 1,
    value = "",
    href = undefined,
    activity = undefined,
    showActivity = false,
    title = "",
    select,
    toggle,
    keydown,
  }: Props = $props();

  function indicateActivity(node: HTMLElement, initial?: TreeActivity) {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const run = (next?: TreeActivity) => {
      clearTimeout(timer);
      const remaining = next ? 1000 - (Date.now() - next.at) : 0;
      node.style.opacity = remaining > 0 ? "1" : "0";
      if (remaining > 0)
        timer = setTimeout(() => {
          node.style.opacity = "0";
        }, remaining);
    };
    run(initial);
    return {
      update: run,
      destroy() {
        clearTimeout(timer);
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
    class:selected
    data-tree-path={path}
    {href}
    role="treeitem"
    style:padding-left={`${depth}rem`}
    tabindex={tabbable ? 0 : -1}
    {title}
    onclick={select}
    onkeydown={keydown}
  >
    <span aria-hidden="true" class="spacer"></span>
    {#if showActivity}<span
        aria-hidden="true"
        class="activity-slot"
        title="Recent settings publication"
        ><span class="activity-dot" use:indicateActivity={activity}
        ></span></span
      >{/if}
    <span class="label">{label}</span>
    {#if summary}<span class="summary">{` (${summary})`}</span>{/if}
    {#if value}
      <span class="separator">{" = "}</span>
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
    class:selected
    data-tree-path={path}
    role="treeitem"
    style:padding-left={`${depth}rem`}
    tabindex={tabbable ? 0 : -1}
    {title}
    onclick={select}
    onkeydown={keydown}
  >
    {#if internal}
      <button
        aria-label={open ? "Collapse" : "Expand"}
        class="toggle"
        tabindex="-1"
        type="button"
        onclick={stopAndToggle}>{open ? "▾" : "▸"}</button
      >
    {:else}
      <span aria-hidden="true" class="spacer"></span>
    {/if}
    {#if showActivity}<span
        aria-hidden="true"
        class="activity-slot"
        title="Recent settings publication"
        ><span class="activity-dot" use:indicateActivity={activity}
        ></span></span
      >{/if}
    {#if href}
      <a class="label" {href} tabindex="-1" onclick={stopAndSelect}>{label}</a>
    {:else}
      <span class="label">{label}</span>
    {/if}
    {#if summary}<span class="summary">{` (${summary})`}</span>{/if}
    {#if value}
      <span class="separator">{" = "}</span>
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
    box-shadow: inset 2px 0 0 var(--selected-mark);
  }

  .label {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .activity-slot {
    align-items: center;
    align-self: stretch;
    display: flex;
    flex: 0 0 var(--caret);
    justify-content: center;
  }

  .activity-dot {
    background: currentColor;
    border-radius: 50%;
    height: var(--activity-size);
    width: var(--activity-size);
    opacity: 0;
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

  .summary {
    color: var(--muted);
    font-size: var(--text-small);
    flex: 0 2 auto;
    max-width: 45%;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: pre;
  }

  .separator {
    flex: none;
    color: var(--muted);
    white-space: pre;
  }
</style>
