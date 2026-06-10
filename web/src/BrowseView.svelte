<svelte:options runes={true} />

<script lang="ts">
  import type { ViewNode } from "./lib/tree-state";
  import type { TreeActions, TreeNodeView } from "./lib/tree-view";
  import SelectedPanel from "./SelectedPanel.svelte";
  import StatusLog from "./StatusLog.svelte";
  import TreeView from "./TreeView.svelte";

  type Props = {
    activePrefix: string;
    discoverHref: string;
    subtreePath: string;
    aliveManifest: { epoch: number; schema_rev: number } | undefined;
    settingsRevision: string;
    status: string;
    error: string;
    treeNodes: Map<string, TreeNodeView>;
    selectedPath: string;
    selected: ViewNode | undefined;
    flashed: Set<string>;
    expanded: Set<string>;
    treeRoot: string;
    editor: string;
    logOpen?: boolean;
    logLines: string[];
    treeActions: TreeActions;
    updateEditor: (value: string) => void;
    submit: () => void;
    resetEditor: () => void;
    focusTree: () => void;
  };

  let {
    activePrefix,
    discoverHref,
    subtreePath,
    aliveManifest,
    settingsRevision,
    status,
    error,
    treeNodes,
    selectedPath,
    selected,
    flashed,
    expanded,
    treeRoot,
    editor,
    logOpen = $bindable(false),
    logLines,
    treeActions,
    updateEditor,
    submit,
    resetEditor,
    focusTree,
  }: Props = $props();
</script>

<section class="browse">
  <div class="top">
    <header>
      <h1><a href={discoverHref}>{activePrefix}</a></h1>
      {#if subtreePath}
        <p>Subtree {subtreePath}</p>
      {/if}
    </header>

    {#if aliveManifest || settingsRevision}
      <section class="meta" aria-label="Static protocol metadata">
        {#if aliveManifest}
          <span>epoch {aliveManifest.epoch}</span>
          <span>schema {aliveManifest.schema_rev}</span>
        {/if}
        {#if settingsRevision}
          <span>rev {settingsRevision}</span>
        {/if}
      </section>
    {/if}

    <div class="tree" aria-label="Schema tree">
      {#if treeNodes.has(treeRoot)}
        <TreeView
          root={treeRoot}
          nodes={treeNodes}
          {selectedPath}
          {flashed}
          {expanded}
          actions={treeActions}
        />
      {:else}
        <p>No schema loaded.</p>
      {/if}
    </div>
  </div>

  <div class="bottom">
    <SelectedPanel node={selected} {editor} {updateEditor} {submit} {resetEditor} {focusTree} />
    <StatusLog {status} {error} bind:open={logOpen} {logLines} />
  </div>
</section>

<style>
  .browse {
    display: grid;
    gap: var(--space);
    /* Keep one scrollable browse region above a stable selected/log panel. */
    grid-template-rows: minmax(0, 1fr) auto;
    height: calc(100svh - 2 * var(--space));
    min-width: 0;
  }

  .top {
    min-height: 0;
    min-width: 0;
    overflow: auto;
  }

  h1 {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  h1 a {
    color: inherit;
    text-decoration-thickness: 1px;
    text-underline-offset: 0.15em;
  }

  .bottom {
    display: grid;
    gap: var(--space-tight);
    min-height: 0;
    min-width: 0;
  }

  .tree {
    min-width: 0;
    overflow: hidden;
  }
  @media (min-width: 761px) {
    .browse {
      height: calc(100dvh - 2 * var(--space));
    }
  }
</style>
