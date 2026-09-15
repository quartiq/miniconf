<svelte:options runes={true} />

<script lang="ts">
  import { displayPath } from "./lib/schema";
  import Metadata from "./Metadata.svelte";
  import type { ViewNode } from "./lib/tree-state";

  type Props = {
    node: ViewNode | undefined;
    path: string;
    canSet: boolean;
    requestMessage: string;
    editor: string;
    editorDirty: boolean;
    editorError: string;
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
    editorError,
    updateEditor,
    submit,
    resetEditor,
    focusTree,
  }: Props = $props();

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
  {#if leaf || editorDirty}
    <section class="editor" aria-label="Leaf editor">
      <div class="value-editor">
        {#if leaf && !node?.present}<p>No value observed</p>{/if}
        {#if !leaf}<p>Leaf unavailable</p>{/if}
        <textarea
          id="leaf-editor"
          aria-label="Setting value"
          aria-keyshortcuts="Control+Enter Meta+Enter Escape"
          data-leaf-editor
          aria-invalid={!!editorError}
          aria-describedby={editorError ? "editor-error" : undefined}
          title="Ctrl/Cmd+Enter sets the value. Esc returns to the tree."
          value={editor}
          oninput={edit}
          onkeydown={maybeSubmit}></textarea>
        {#if editorError}<p id="editor-error" role="alert">
            {editorError}
          </p>{/if}
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
          title={node?.present
            ? "Replace your edits with the latest device value; nothing is sent."
            : "Discard your edits; no device value has been received."}
          type="button"
          onclick={resetEditor}>Revert</button
        >
      </div>
      {#if requestMessage}<p class="request" role="status">
          {requestMessage}
        </p>{/if}
    </section>
  {/if}
  <section class="schema-body" aria-label="Schema">
    {#if node}<div class="meta">Kind: {node.kind}</div>{/if}
    {#if node?.sem !== undefined}<Metadata
        label="Semantics"
        heading={false}
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
  </section>
</section>

<style>
  .value-editor {
    min-width: 0;
  }
  h2 {
    overflow-wrap: anywhere;
  }

  .selected {
    display: grid;
    grid-auto-rows: max-content;
    gap: 0;
    min-width: 0;
    align-content: start;
    overflow: auto;
  }

  .schema-body {
    margin-top: var(--space);
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
