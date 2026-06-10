<svelte:options runes={true} />

<script lang="ts">
  import { formatSchemaMetadata } from "./lib/schema";
  import type { ViewNode } from "./lib/tree-state";

  type Props = {
    node: ViewNode | undefined;
    editor?: string;
    updateEditor: (value: string) => void;
    submit: () => void;
    resetEditor: () => void;
    focusTree: () => void;
  };

  let {
    node,
    editor = "null",
    updateEditor,
    submit,
    resetEditor,
    focusTree,
  }: Props = $props();

  let schemaOpen = $state(true);
  let metadata = $derived(node ? formatSchemaMetadata(node) : "");
  let leaf = $derived(node?.kind === "leaf");

  function edit(event: Event) {
    updateEditor((event.currentTarget as HTMLTextAreaElement).value);
  }

  function maybeSubmit(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
      event.preventDefault();
      submit();
    } else if (event.key === "Escape") {
      event.preventDefault();
      focusTree();
    }
  }
</script>

<section class="selected" aria-label="Selected item">
  <details class="schema" bind:open={schemaOpen}>
    <summary>
      <span aria-hidden="true" class="caret">{schemaOpen ? "▾" : "▸"}</span>
      <span>{node?.path ?? ""}</span>
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
    {#if leaf}
      <textarea
        aria-keyshortcuts="Control+Enter Meta+Enter Escape"
        data-leaf-editor
        title="Ctrl/Cmd+Enter sets the value. Esc returns to the tree."
        value={editor}
        oninput={edit}
        onkeydown={maybeSubmit}
      ></textarea>
      <div class="actions">
        <button
          aria-keyshortcuts="Control+Enter Meta+Enter"
          title="Ctrl/Cmd+Enter"
          type="button"
          onclick={submit}
        >Set</button>
        <!-- Reset intentionally has no keyboard shortcut: it discards the draft. -->
        <button
          title="Reset the draft to the current value"
          type="button"
          onclick={resetEditor}
        >Reset</button>
      </div>
    {:else}
      <p>No leaf selected.</p>
    {/if}
  </section>
</section>

<style>
  .selected {
    border-block: 1px solid var(--border);
    display: grid;
    gap: 0;
    min-width: 0;
    padding-block: var(--space-tight);
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
