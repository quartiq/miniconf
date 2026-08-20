<svelte:options runes={true} />

<script lang="ts">
  import type { ViewNode } from "./lib/tree-state";
  import type { TreeActions, TreeNodeView } from "./lib/tree-view";
  import SelectedPanel from "./SelectedPanel.svelte";
  import StatusLog from "./StatusLog.svelte";
  import TreeView from "./TreeView.svelte";

  type Props = {
    broker: string;
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
    activity: Map<string, import("./lib/tree-view").TreeActivity>;
    expanded: Set<string>;
    treeRoot: string;
    editor: string;
    editorDirty: boolean;
    editorStale: boolean;
    logOpen?: boolean;
    logLines: string[];
    treeActions: TreeActions;
    updateEditor: (value: string) => void;
    submit: () => void;
    resetEditor: () => void;
    focusTree: () => void;
  };

  let {
    broker,
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
    activity,
    expanded,
    treeRoot,
    editor,
    editorDirty,
    editorStale,
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
  <header class="app-header panel">
    <a class="back" href={discoverHref} aria-label="Change connection" title="Change connection">←</a>
    <div class="context">
      <h1 title={activePrefix}>{activePrefix}</h1>
      <div class="meta">
        <span title={broker}>{broker}</span>
        {#if subtreePath}<span>subtree {subtreePath}</span>{/if}
        {#if aliveManifest}
          <span>epoch {aliveManifest.epoch}</span>
          <span>schema {aliveManifest.schema_rev}</span>
        {/if}
        {#if settingsRevision}<span>rev {settingsRevision}</span>{/if}
      </div>
    </div>
    <div class="connection-state" role="status" title={error || status}>
      <span>{status}</span>
      {#if error}<strong>{error}</strong>{/if}
    </div>
  </header>

  <div class="workspace">
    <section class="tree panel" aria-labelledby="settings-title">
      <h2 id="settings-title">Settings</h2>
      {#if treeNodes.has(treeRoot)}
        <TreeView
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

    <SelectedPanel node={selected} {editor} {editorDirty} {editorStale} {updateEditor} {submit} {resetEditor} {focusTree} />
  </div>

  <StatusLog {status} {error} bind:open={logOpen} {logLines} />
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
    display: grid;
    gap: var(--space);
    grid-template-columns: auto minmax(0, 1fr) auto;
    min-width: 0;
  }

  .back {
    color: inherit;
    line-height: var(--line);
    text-decoration: none;
  }

  .context,
  .connection-state {
    min-width: 0;
  }

  h1,
  .connection-state span,
  .connection-state strong {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .connection-state {
    color: var(--muted);
    max-width: 30vw;
    text-align: right;
  }

  .connection-state strong {
    color: var(--error);
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
    }
  }

  @media (max-width: 760px) {
    .browse {
      height: auto;
    }

    .app-header {
      grid-template-columns: auto minmax(0, 1fr);
    }

    .workspace {
      grid-template-columns: 1fr;
    }

    .connection-state {
      grid-column: 2;
      max-width: none;
      text-align: left;
    }

    .tree {
      max-height: 52svh;
    }
  }
</style>
