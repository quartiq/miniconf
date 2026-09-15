<svelte:options runes={true} />

<script lang="ts">
  import type {
    TreeActions,
    TreeActivity,
    TreeNodeView,
  } from "./lib/tree-view";
  import TreeItem from "./TreeItem.svelte";
  import TreeRow from "./TreeRow.svelte";

  type Props = {
    node: TreeNodeView;
    nodes: Map<string, TreeNodeView>;
    selectedPath: string;
    tabStop: string;
    activity?: Map<string, TreeActivity>;
    showActivity?: boolean;
    expanded: Set<string>;
    actions: TreeActions;
    depth?: number;
    index?: number;
    size?: number;
  };

  let {
    node,
    nodes,
    selectedPath,
    tabStop,
    activity = new Map(),
    showActivity = false,
    expanded,
    actions,
    depth = 0,
    index = 1,
    size = 1,
  }: Props = $props();

  let selected = $derived(node.path === selectedPath);
  let internal = $derived(node.children.length > 0);
  let open = $derived(expanded.has(node.path));
  let rowActivity = $derived(activity.get(node.path));

  function select() {
    actions.select(node.path);
  }

  function toggle() {
    if (internal) {
      actions.open(node.path, !open);
    }
  }

  function keydown(event: KeyboardEvent) {
    // Follow the usual tree-view model: arrows navigate structure, Space folds,
    // Enter activates, and focus stays on the selected row.
    switch (event.key) {
      case "Enter":
        if (!node.href || internal) {
          event.preventDefault();
          if (actions.activate) {
            actions.activate(node, internal, open);
          } else if (internal) {
            toggle();
          } else {
            select();
          }
        }
        break;
      case " ":
        event.preventDefault();
        if (internal) {
          toggle();
        } else {
          select();
        }
        break;
      case "Escape":
        if (internal && open) {
          event.preventDefault();
          actions.open(node.path, false);
        }
        break;
      case "ArrowRight":
        if (!internal) {
          break;
        }
        event.preventDefault();
        if (!open) {
          actions.open(node.path, true);
        } else {
          actions.key(node, "child");
        }
        break;
      case "ArrowLeft":
        event.preventDefault();
        if (internal && open) {
          actions.open(node.path, false);
        } else {
          actions.key(node, "parent");
        }
        break;
      case "ArrowDown":
        event.preventDefault();
        actions.key(node, "next");
        break;
      case "ArrowUp":
        event.preventDefault();
        actions.key(node, "previous");
        break;
      case "PageDown":
        event.preventDefault();
        actions.key(node, "pageNext", 10);
        break;
      case "PageUp":
        event.preventDefault();
        actions.key(node, "pagePrevious", 10);
        break;
      case "Home":
        event.preventDefault();
        actions.key(node, "first");
        break;
      case "End":
        event.preventDefault();
        actions.key(node, "last");
        break;
    }
  }
</script>

<li>
  <TreeRow
    path={node.path}
    label={node.label}
    summary={node.summary}
    value={node.value ?? ""}
    {selected}
    tabbable={node.path === tabStop}
    {internal}
    {open}
    {depth}
    level={depth + 1}
    posinset={index}
    setsize={size}
    href={node.href}
    activity={rowActivity}
    {showActivity}
    title={node.title ?? node.path}
    {select}
    {toggle}
    {keydown}
  />

  {#if internal && open}
    <ul role="group">
      {#each node.children as childPath, childIndex (childPath)}
        {@const child = nodes.get(childPath)}
        {#if child}
          <TreeItem
            node={child}
            {nodes}
            {selectedPath}
            {tabStop}
            {activity}
            {showActivity}
            {expanded}
            {actions}
            depth={depth + 1}
            index={childIndex + 1}
            size={node.children.length}
          />
        {/if}
      {/each}
    </ul>
  {/if}
</li>

<style>
  li {
    list-style: none;
    min-width: 0;
  }

  ul {
    margin: 0;
    min-width: 0;
    padding: 0;
  }
</style>
