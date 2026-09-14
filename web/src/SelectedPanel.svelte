<svelte:options runes={true} />

<script lang="ts">
  import { displayPath, schemaSummary } from "./lib/schema";
  import Metadata from "./Metadata.svelte";
  import type { ViewNode } from "./lib/tree-state";

  type Props = {
    node: ViewNode | undefined;
    path: string;
    canSet: boolean;
    requestMessage: string;
    editor: string;
    editorDirty: boolean;
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
    updateEditor,
    submit,
    resetEditor,
    focusTree,
  }: Props = $props();

  let schemaOpen = $state(false);
  let leaf = $derived(node?.kind === "leaf");
  let differs = $derived(editor !== (node?.value ?? ""));

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

{#snippet schemaDetails()}
  <div class="schema-body">
    {#if node}<div class="meta">Kind: {node.kind}</div>{/if}
    {#if node?.sem !== undefined}<Metadata
        label="Semantics"
        value={node.sem}
      />{/if}
    {#if node?.edge !== undefined}<Metadata
        label="Edge metadata"
        value={node.edge}
      />{/if}
    {#if node?.node !== undefined}<Metadata
        label="Node metadata"
        value={node.node}
      />{/if}
    {#if node?.sem === undefined && node?.edge === undefined && node?.node === undefined}<p
      >
        No schema metadata.
      </p>{/if}
  </div>
{/snippet}

<section class="selected panel" aria-label="Selected item">
  <h2>{displayPath(path)}</h2>
  {#if node && !leaf}
    <section class="schema" aria-label="Schema">
      <h3>Schema</h3>
      {@render schemaDetails()}
    </section>
  {:else}
    <details class="schema" bind:open={schemaOpen}>
      <summary>
        <span aria-hidden="true" class="caret">{schemaOpen ? "▾" : "▸"}</span>
        <span
          >Schema{!schemaOpen && node ? ` · ${schemaSummary(node)}` : ""}</span
        >
      </summary>
      {@render schemaDetails()}
    </details>
  {/if}
  {#if leaf || editorDirty}
    <section class="editor" aria-label="Leaf editor">
      <div class="value-editor">
        <span class="meta">Device value</span>
        <pre class="device-value">{node?.value ?? "No value observed"}</pre>
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
          disabled={!differs}
          title="Reset the draft to the current value"
          type="button"
          onclick={resetEditor}
          >{node?.present ? "Use device value" : "Clear draft"}</button
        >
      </div>
      {#if requestMessage}<p class="request" role="status">
          {requestMessage}
        </p>{/if}
    </section>
  {/if}
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
  h2 {
    overflow-wrap: anywhere;
  }
  h3 {
    font-size: var(--text);
    font-weight: 600;
    margin: 0;
    line-height: var(--line);
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
    margin-top: var(--space-tight);
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
    grid-template-columns: minmax(0, 1fr);
    min-block-size: calc(4 * var(--line));
  }

  .editor p {
    color: var(--muted);
    margin: 0;
  }

  .actions {
    align-items: baseline;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space);
  }

  .actions button {
    width: auto;
    white-space: nowrap;
  }

  @media (max-width: 760px) {
    .selected {
      overflow: visible;
    }
  }
</style>
