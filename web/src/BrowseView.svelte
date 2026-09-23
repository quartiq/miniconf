<svelte:options runes={true} />

<script lang="ts">
  import type { BrowseModel } from "./lib/browse-model.svelte";
  import SelectedPanel from "./SelectedPanel.svelte";
  import StatusLog from "./StatusLog.svelte";
  import TreeView from "./TreeView.svelte";
  import BuildIdentity from "./BuildIdentity.svelte";

  type Props = {
    model: BrowseModel;
    broker: string;
    activePrefix: string;
    discoverHref: string;
    subtreePath: string;
    status: { text: string; failed: boolean };
    retryable: boolean;
    retry: () => void;
    logOpen?: boolean;
    logLines: string[];
  };
  let {
    model,
    broker,
    activePrefix,
    discoverHref,
    subtreePath,
    status,
    retryable,
    retry,
    logOpen = $bindable(false),
    logLines,
  }: Props = $props();
  let tree = $state<{ focus: (path: string) => Promise<void> }>();
  let panel = $state<{ focus: () => Promise<void> }>();
  let settingCount = $derived.by(() => {
    let count = 0;
    for (const node of model.state.tree.values())
      if (node.kind === "leaf") count++;
    return count;
  });
  let diagnostics = $derived(
    [
      ...(model.alive
        ? [
            `Schema revision ${model.alive.schema_rev}`,
            `Epoch ${model.alive.epoch}`,
          ]
        : []),
      ...(model.revision ? [`Last settings revision ${model.revision}`] : []),
    ].join("\n"),
  );
</script>

<section class="browse">
  <header class="app-header panel">
    <a class="back" href={discoverHref} title={`Show devices on ${broker}`}
      >{broker}</a
    >
    <div class="context">
      <h1 title={diagnostics}>{activePrefix}</h1>
      {#if model.state.tree.has(model.state.root)}<span
          class="identity-details"
          title="Number of settings in the displayed schema; includes leaves whose values have not been observed."
          >{`${settingCount} ${settingCount === 1 ? "setting" : "settings"}${subtreePath ? " in subtree" : ""}`}</span
        >{/if}
      {#if subtreePath}<span class="identity-details"
          >subtree {subtreePath}</span
        >{/if}
    </div>
    <div class="connection-state">
      <div class="status">
        <div role="status" class:failed={status.failed} title={status.text}>
          {status.text}
        </div>
        {#if retryable}<button type="button" onclick={retry}>Retry</button>{/if}
        <span class="build-separator" aria-hidden="true">·</span>
        <BuildIdentity />
      </div>
      <div class="prune-action">
        {#if model.pruning.count}
          <button
            class="prune"
            type="button"
            disabled={!model.canPrune}
            onclick={() => void model.prune()}
            title={`Clear ${model.pruning.count} observed stale retained messages from this device’s broker topics. Covers the entire device prefix; preserves valid settings.`}
            >Prune ({model.pruning.count})</button
          >
        {/if}
      </div>
      {#if model.pruning.coverageWarning}<span
          class="meta coverage"
          title={model.pruning.coverageWarning}>Partial pruning coverage</span
        >{/if}
    </div>
  </header>

  <div class="workspace">
    <section class="tree panel" aria-labelledby="settings-title">
      <h2 id="settings-title">Settings</h2>
      {#if model.state.tree.has(model.state.root)}
        <TreeView
          bind:this={tree}
          label="Settings"
          root={model.state.root}
          nodes={model.state.tree}
          selectedPath={model.state.selectedPath}
          activity={model.activity}
          expanded={model.state.expanded}
          actions={{
            select: (path) => model.select(path),
            open: (path, open) => model.setExpanded(path, open),
            key: (node, direction, step) =>
              model.navigate(node.path, direction, step),
            activate: (node, internal, open) => {
              if (internal) model.setExpanded(node.path, !open);
              else {
                model.select(node.path);
                if (model.selected?.kind === "leaf") void panel?.focus();
              }
            },
          }}
        />
      {:else}
        <p>No schema loaded.</p>
      {/if}
    </section>

    <SelectedPanel
      bind:this={panel}
      node={model.selected}
      path={model.state.selectedPath}
      canSet={model.canSet}
      editor={model.editor}
      editorDirty={model.dirty}
      editorError={model.editorError}
      updateEditor={(text) => model.edit(text)}
      submit={() => void model.submit()}
      revert={() => model.revert()}
      focusTree={() => void tree?.focus(model.state.selectedPath)}
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

  h1 {
    margin: 0;
    line-height: var(--line);
    flex-shrink: 0;
    max-width: 100%;
    display: block;
    overflow-wrap: anywhere;
  }

  .connection-state {
    min-width: 0;
    flex: 1 0 22ch;
    max-width: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-tight) var(--space);
    overflow-wrap: anywhere;
  }
  .prune-action {
    min-width: 9ch;
    height: var(--line);
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
    gap: var(--space-tight);
    justify-content: flex-end;
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
  .build-separator {
    color: var(--muted);
    font-size: var(--text-small);
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

  .status [role="status"].failed {
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
    .connection-state {
      flex-basis: 100%;
    }

    .status {
      justify-content: flex-start;
    }

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
