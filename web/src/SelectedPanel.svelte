<svelte:options runes={true} />

<script lang="ts">
  import { displayPath, formatSchemaMetadata } from "./lib/schema";
  import type { ViewNode } from "./lib/tree-state";

  type Props = {
    node: ViewNode | undefined;
    path: string;
    canSet: boolean;
    requestMessage: string;
    editor: string;
    editorDirty: boolean;
    editorStale: boolean;
    updateEditor: (value: string) => void;
    submit: () => void;
    resetEditor: () => void;
    focusTree: () => void;
  };

  let {
    node,
    path,
    canSet,
    requestMessage,
    editor,
    editorDirty,
    editorStale,
    updateEditor,
    submit,
    resetEditor,
    focusTree,
  }: Props = $props();

  let schemaOpen = $state(false);
  let metadata = $derived(node ? formatSchemaMetadata(node) : "");
  let leaf = $derived(node?.kind === "leaf");

  function edit(event: Event) {
    updateEditor((event.currentTarget as HTMLTextAreaElement).value);
  }

  function maybeSubmit(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
      event.preventDefault();
      if (canSet) submit();
    } else if (event.key === "Escape") {
      event.preventDefault();
      focusTree();
    }
  }
</script>

<section class="selected panel" aria-label="Selected item">
  <h2>{displayPath(path)}</h2>
  <details class="schema" bind:open={schemaOpen}>
    <summary>
      <span aria-hidden="true" class="caret">{schemaOpen ? "▾" : "▸"}</span>
      <span>Schema</span>
    </summary>
    <div class="schema-body">
      {#if metadata}
        <pre>{metadata}</pre>
      {:else}
        <p>No schema metadata.</p>
      {/if}
    </div>
  </details>
  <section class="editor" aria-label="Leaf editor">
    {#if leaf || editorDirty}
      <div class="value-editor">
        {#if editorDirty || requestMessage || !node?.present}
          <span class="meta">Device value</span>
          <pre class="device-value">{node?.value ?? "No value observed"}</pre>
        {/if}
        {#if !leaf}<p>Leaf unavailable</p>{/if}
        <label for="leaf-editor">Draft</label>
        <textarea
          id="leaf-editor"
          aria-keyshortcuts="Control+Enter Meta+Enter Escape"
          data-leaf-editor
          title="Ctrl/Cmd+Enter sets the value. Esc returns to the tree."
          value={editor}
          oninput={edit}
          onkeydown={maybeSubmit}></textarea>
      </div>
      <div class="actions">
        <button
          aria-keyshortcuts="Control+Enter Meta+Enter"
          title="Ctrl/Cmd+Enter"
          type="button"
          disabled={!canSet}
          onclick={submit}>Set</button
        >
        <!-- Reset intentionally has no keyboard shortcut: it discards the draft. -->
        <button
          disabled={!editorDirty}
          title="Reset the draft to the current value"
          type="button"
          onclick={resetEditor}
          >{node?.present ? "Use device value" : "Clear draft"}</button
        >
        {#if editorStale}
          <span class="stale">Device value updated</span>
        {:else if editorDirty}
          <span class="draft">Edited</span>
        {/if}
      </div>
      {#if requestMessage}<p class="request" role="status">
          {requestMessage}
        </p>{/if}
    {:else}
      <p>No leaf selected.</p>
    {/if}
  </section>
</section>

<style>
  .value-editor {
    min-width: 0;
  }
  .device-value {
    max-height: calc(4 * var(--line));
    overflow: auto;
    margin: 0 0 var(--space-tight);
  }
  .request {
    grid-column: 1 / -1;
  }

  .selected {
    display: grid;
    gap: 0;
    min-width: 0;
    align-content: start;
    overflow: auto;
  }

  .schema summary {
    align-items: baseline;
    cursor: pointer;
    display: flex;
    gap: 0;
    line-height: var(--line);
    list-style: none;
    min-height: var(--line);
  }

  .schema summary::-webkit-details-marker {
    display: none;
  }

  .caret {
    display: inline-block;
    flex: 0 0 var(--caret);
    line-height: var(--line);
  }

  .schema-body {
    block-size: calc(4 * var(--line));
    margin-top: var(--space-tight);
    overflow: auto;
  }

  .schema-body p {
    margin: 0;
  }

  textarea {
    block-size: calc(4 * var(--line));
    display: block;
    font: inherit;
    overflow: auto;
    resize: vertical;
    width: 100%;
  }

  .editor {
    align-items: start;
    display: grid;
    gap: var(--space-tight);
    grid-template-columns: minmax(0, 1fr) auto;
    min-block-size: calc(4 * var(--line));
  }

  .editor p {
    color: var(--muted);
    margin: 0;
  }

  .actions {
    align-items: baseline;
    display: flex;
    flex-direction: column;
    gap: var(--space);
  }

  .draft,
  .stale {
    font-size: var(--text-small);
  }

  .draft {
    color: var(--muted);
  }

  .stale {
    color: var(--warn);
  }

  @media (max-width: 760px) {
    .editor {
      grid-template-columns: 1fr;
    }

    .actions {
      flex-direction: row;
    }

    .actions button {
      flex: 1 1 0;
      width: auto;
    }
  }
</style>
