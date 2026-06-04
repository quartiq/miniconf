<script lang="ts">
  import type { ViewNode } from "./lib/tree-state";
  import type { TreeActions, TreeNodeView } from "./lib/tree-view";
  import SelectedPanel from "./SelectedPanel.svelte";
  import StatusLog from "./StatusLog.svelte";
  import TreeItem from "./TreeItem.svelte";

  export let activePrefix: string;
  export let discoverHref: string;
  export let subtreePath: string;
  export let aliveManifest: { epoch: number; schema_rev: number } | undefined;
  export let settingsRevision: string;
  export let status: string;
  export let error: string;
  export let rootNode: ViewNode | undefined;
  export let treeNodes: Map<string, TreeNodeView>;
  export let selectedPath: string;
  export let selected: ViewNode | undefined;
  export let flashed: Set<string>;
  export let expanded: Set<string>;
  export let treeRoot: string;
  export let editor: string;
  export let logOpen: boolean;
  export let logLines: string[];
  export let treeActions: TreeActions;
  export let updateEditor: (value: string) => void;
  export let submit: () => void;
  export let resetEditor: () => void;
  export let focusTree: () => void;
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
      {#if rootNode}
        <ul role="tree">
          <TreeItem
            node={treeNodes.get(treeRoot)!}
            nodes={treeNodes}
            {selectedPath}
            {flashed}
            {expanded}
            actions={treeActions}
          />
        </ul>
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
    height: calc(100dvh - 2 * var(--space));
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

  .tree ul {
    margin: 0;
    min-width: 0;
    padding: 0;
  }
</style>
