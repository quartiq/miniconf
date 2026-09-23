<script lang="ts">
  const commit = __BUILD_COMMIT__;
  const url = /^[0-9a-f]{40}$/i.test(commit)
    ? `https://github.com/quartiq/miniconf/commit/${commit}`
    : undefined;
</script>

{#if url}
  <a
    class="build-id"
    href={url}
    target="_blank"
    rel="noreferrer"
    title={`Open source commit ${commit}`}>build {commit.slice(0, 8)}</a
  >
{:else}
  <span class="build-id" title="No source commit was supplied at build time"
    >local build</span
  >
{/if}

<style>
  .build-id {
    color: var(--muted);
    font-size: var(--text-small);
    flex: none;
    white-space: nowrap;
  }
</style>
