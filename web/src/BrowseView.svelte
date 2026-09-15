<svelte:options runes={true} />

<script lang="ts">
  import type { PruningState } from "./lib/backend";
  import type { ViewNode } from "./lib/tree-state";
  import type { TreeActions, TreeNodeView } from "./lib/tree-view";
  import SelectedPanel from "./SelectedPanel.svelte";
  import StatusLog from "./StatusLog.svelte";
  import TreeView from "./TreeView.svelte";

  type Props = {
    pruning: PruningState;
    canPrune: boolean;
    prune: () => void;
    broker: string;
    activePrefix: string;
    discoverHref: string;
    subtreePath: string;
    aliveManifest: { epoch: number; schema_rev: number } | undefined;
    settingsRevision: string;
    status: string;
    error: string;
    retryable: boolean;
    treeNodes: Map<string, TreeNodeView>;
    selectedPath: string;
    selected: ViewNode | undefined;
    activity: Map<string, import("./lib/tree-view").TreeActivity>;
    expanded: Set<string>;
    treeRoot: string;
    canSet: boolean;
    editor: string;
    editorDirty: boolean;
    editorError: string;
    logOpen?: boolean;
    logLines: string[];
    treeActions: TreeActions;
    updateEditor: (value: string) => void;
    submit: () => void;
    resetEditor: () => void;
    focusTree: () => void;
    retry: () => void;
  };

  let {
    pruning,
    canPrune,
    prune,
    broker,
    activePrefix,
    discoverHref,
    subtreePath,
    aliveManifest,
    settingsRevision,
    status,
    error,
    retryable,
    treeNodes,
    selectedPath,
    selected,
    activity,
    expanded,
    treeRoot,
    canSet,
    editor,
    editorDirty,
    editorError,
    logOpen = $bindable(false),
    logLines,
    treeActions,
    updateEditor,
    submit,
    resetEditor,
    focusTree,
    retry,
  }: Props = $props();
</script>

<section class="browse">
  <header class="app-header panel">
    <a class="back" href={discoverHref} title={`Show devices on ${broker}`}
      >{broker}</a
    >
    <div class="context">
      <h1>{activePrefix}</h1>
      {#if aliveManifest}<span class="identity-details"
          >schema {aliveManifest.schema_rev}</span
        >{/if}
      {#if subtreePath}<span class="identity-details"
          >subtree {subtreePath}</span
        >{/if}
      <div class="identity-details">
        {#if aliveManifest}epoch {aliveManifest.epoch}{/if}
        {#if settingsRevision}
          · last publication rev {settingsRevision}{/if}
      </div>
    </div>
    <div class="connection-state">
      <div class="status">
        <div role="status" title={error || status}>
          {#if error}<strong>{status}: {error}</strong>{:else}<span
              >{status}</span
            >{/if}
        </div>
        {#if retryable}<button type="button" onclick={retry}>Retry</button>{/if}
      </div>
      <div class="prune-action">
        {#if pruning.count}
          <button
            class="prune"
            type="button"
            disabled={!canPrune}
            onclick={prune}
            title={`Clear ${pruning.count} observed stale retained messages from this device’s broker topics. Covers the entire device prefix; preserves valid settings.`}
            >Prune ({pruning.count})</button
          >
        {/if}
      </div>
      {#if pruning.coverageWarning}<span
          class="meta coverage"
          title={pruning.coverageWarning}>Partial pruning coverage</span
        >{/if}
    </div>
  </header>

  <div class="workspace">
    <section class="tree panel" aria-labelledby="settings-title">
      <h2 id="settings-title">Settings</h2>
      {#if treeNodes.has(treeRoot)}
        <TreeView
          label="Settings"
          root={treeRoot}
          nodes={treeNodes}
          {selectedPath}
          {activity}
          {expanded}
          actions={treeActions}
        />
      {:else}
        <p>No schema loaded.</p>
      {/if}
    </section>

    <SelectedPanel
      node={selected}
      path={selectedPath}
      {canSet}
      {editor}
      {editorDirty}
      {editorError}
      {updateEditor}
      {submit}
      {resetEditor}
      {focusTree}
    />
  </div>

  <StatusLog status="Log" bind:open={logOpen} {logLines} />
</section>

<style>
  .browse {
    display: grid;
    gap: var(--space);
    grid-template-rows: auto minmax(0, 1fr) auto;
    height: calc(100svh - 2 * var(--space));
    min-width: 0;
  }

  .app-header {
    align-items: center;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-tight) var(--space);
    min-width: 0;
  }

  .back {
    color: inherit;
    line-height: var(--line);
    text-decoration: none;
    max-width: 100%;
    min-width: 0;
    overflow-wrap: anywhere;
    flex-shrink: 0;
  }

  .connection-state {
    min-width: 0;
  }

  h1 {
    margin: 0;
    line-height: var(--line);
    flex-shrink: 0;
    max-width: 100%;
    display: block;
    overflow-wrap: anywhere;
  }

  .connection-state {
    flex: 1 0 22ch;
    max-width: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-tight) var(--space);
    overflow-wrap: anywhere;
  }
  .connection-state button {
    width: auto;
  }
  .prune-action {
    min-width: 9ch;
    height: calc(var(--line) + var(--space-tight));
  }
  .prune-action button {
    width: 100%;
    height: 100%;
    white-space: nowrap;
  }
  .status {
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: var(--space);
    line-height: var(--line);
  }
  .status [role="status"] {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .coverage {
    grid-column: 1 / -1;
  }
  .context {
    display: contents;
  }
  .identity-details {
    overflow-wrap: anywhere;
    color: var(--muted);
    font-size: var(--text-small);
    min-width: 0;
  }

  .connection-state strong {
    color: var(--error);
    white-space: normal;
  }

  .workspace {
    display: grid;
    gap: var(--space);
    grid-template-columns: minmax(0, 3fr) minmax(18rem, 2fr);
    min-height: 0;
    min-width: 0;
  }

  .tree {
    min-height: 0;
    min-width: 0;
    overflow: auto;
  }

  @media (min-width: 761px) {
    .browse {
      height: calc(100dvh - 2 * var(--space));
      grid-template-rows: auto minmax(calc(12 * var(--line)), 1fr) auto;
    }
  }

  @media (max-width: 760px) {
    .browse {
      height: auto;
    }

    .workspace {
      grid-template-columns: 1fr;
    }

    .tree {
      max-height: 52svh;
    }
  }
</style>
