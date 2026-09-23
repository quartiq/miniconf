import type { AliveManifest, PrefixSession, PruningState } from "./backend";
import { displayPath, type Schema } from "./schema";
import type { Settings, SettingsCommit } from "./settings-mirror";
import {
  updateActivity,
  cuePaths,
  revealPresentSettings,
  treeSnapshot,
  type ViewNode,
} from "./tree-state";
import {
  movePath,
  toggleExpansion,
  visibleTreePaths,
  type NavDirection,
} from "./tree-navigation";
import type { TreeActivity } from "./tree-view";

type Action = "Set" | "Prune";
type ActionResult = { text: string; failed: boolean; responseMs?: number };
// Capture both together: a completed command belongs to the connection it began on.
type CommandSession = {
  session: Pick<PrefixSession, "set" | "prune">;
  signal: AbortSignal;
};

// An undefined draft follows observed values; any string, including empty, is an edit.
type BrowseState = {
  schema: Schema | undefined;
  settings: Settings;
  root: string;
  draft: string | undefined;
  expanded: Set<string>;
  selectedPath: string;
  userClosed: Set<string>;
  tree: Map<string, ViewNode>;
};

export type BrowseMemory = Pick<
  BrowseState,
  "expanded" | "selectedPath" | "userClosed"
>;

// Remember recent routes without retaining every device/subtree visited in a tab.
export function rememberRoute(
  memory: Map<string, BrowseMemory>,
  key: string,
  state: BrowseMemory,
): void {
  memory.delete(key);
  memory.set(key, {
    expanded: new Set(state.expanded),
    selectedPath: state.selectedPath,
    userClosed: new Set(state.userClosed),
  });
  if (memory.size > 32) memory.delete(memory.keys().next().value!);
}

function emptyState(): BrowseState {
  return {
    schema: undefined,
    settings: new Map(),
    root: "",
    draft: undefined,
    expanded: new Set(),
    selectedPath: "",
    userClosed: new Set(),
    tree: new Map(),
  };
}

export class BrowseModel {
  private data = $state.raw(emptyState());
  get state(): Readonly<BrowseState> {
    return this.data;
  }
  alive = $state<AliveManifest>();
  revision = $state("");
  pruning = $state<PruningState>({ count: 0, coverageWarning: "" });
  activity = $state.raw(new Map<string, TreeActivity>());
  pending = $state.raw(new Set<Action>());
  result = $state<ActionResult>();
  private validation = $state<{
    path: string;
    text: string;
    message: string;
  }>();

  selected = $derived(this.data.tree.get(this.data.selectedPath));
  editor = $derived(this.data.draft ?? this.selected?.value ?? "");
  dirty = $derived(
    this.data.draft !== undefined && this.data.draft !== this.selected?.value,
  );
  editorError = $derived(
    this.validation?.path === this.data.selectedPath &&
      this.validation.text === this.editor
      ? this.validation.message
      : "",
  );
  get canSet(): boolean {
    return (
      !!this.getSession() &&
      this.selected?.kind === "leaf" &&
      !this.pending.has("Set")
    );
  }
  get canPrune(): boolean {
    return !!this.getSession() && !this.pending.has("Prune");
  }

  constructor(
    private readonly getSession: () => CommandSession | undefined,
    private readonly log: (event: string, detail: string) => void,
  ) {}

  reset(preserve: boolean): void {
    this.alive = undefined;
    this.revision = "";
    this.pruning = { count: 0, coverageWarning: "" };
    if (preserve)
      this.commitSettings({
        settings: new Map(),
        touched: new Set(this.data.settings.keys()),
        activity: new Set(),
      });
    else this.data = emptyState();
    this.pending = new Set();
    this.result = undefined;
    this.validation = undefined;
    this.activity = new Map();
  }

  observeAlive(alive: AliveManifest | undefined): void {
    this.alive = alive;
    if (!alive) {
      this.revision = "";
      if (!this.result?.failed) this.result = undefined;
    }
  }

  loadSchema(
    schema: Schema,
    subtreePath: string,
    memory: BrowseMemory = this.data,
  ): void {
    const root = schema.path(subtreePath);
    const tree = treeSnapshot(schema, root, this.data.settings);
    const branches = new Set(
      [...tree.values()]
        .filter(({ children }) => children.length)
        .map(({ path }) => path),
    );
    this.data = {
      ...this.data,
      schema,
      root,
      tree,
      selectedPath:
        this.data.draft !== undefined || tree.has(memory.selectedPath)
          ? memory.selectedPath
          : root,
      expanded: new Set(
        [...memory.expanded].filter((path) => branches.has(path)),
      ),
      userClosed: new Set(
        [...memory.userClosed].filter((path) => branches.has(path)),
      ),
    };
    this.activity = updateActivity(this.activity, [], this.data.tree);
  }

  commitSettings(commit: SettingsCommit): void {
    if (commit.activity.size) {
      const changed = [...commit.activity].filter(
        (path) =>
          this.data.settings.has(path) &&
          this.data.settings.get(path) !== commit.settings.get(path),
      ).length;
      this.log(
        "settings",
        `${changed} changed · ${commit.activity.size} observed`,
      );
    }
    const { settings, touched, activity, rev } = commit;
    let tree = this.data.tree;
    for (const path of touched) {
      const node = tree.get(path);
      const value = settings.get(path);
      if (node?.kind !== "leaf" || node.value === value) continue;
      if (tree === this.data.tree) tree = new Map(tree);
      tree.set(path, { ...node, value });
    }
    this.data = {
      ...this.data,
      settings,
      tree,
      expanded: revealPresentSettings(
        this.data.expanded,
        this.data.userClosed,
        touched,
        settings,
        this.data.root,
      ),
    };
    if (touched.size) this.revision = rev ?? this.revision;
    this.activity = updateActivity(
      this.activity,
      cuePaths(activity, this.data.root),
      tree,
    );
  }

  setExpanded(path: string, open: boolean): void {
    this.data = {
      ...this.data,
      ...toggleExpansion(this.data.expanded, this.data.userClosed, path, open),
    };
  }
  select(path: string): void {
    if (path !== this.data.selectedPath)
      this.data = { ...this.data, selectedPath: path, draft: undefined };
  }
  navigate(path: string, direction: NavDirection, step?: number): string {
    const next = movePath(
      visibleTreePaths(this.data.root, this.data.tree, this.data.expanded),
      path,
      direction,
      this.data.tree,
      step,
    );
    this.select(next);
    return next;
  }
  edit(text: string): void {
    this.data = {
      ...this.data,
      draft: text === this.selected?.value ? undefined : text,
    };
  }
  revert(): void {
    this.data = { ...this.data, draft: undefined };
  }

  async submit(): Promise<void> {
    if (!this.canSet || !this.selected) return;
    const path = this.selected.path;
    const text = this.editor;
    this.result = undefined;
    try {
      JSON.parse(text);
    } catch (error) {
      this.validation = {
        path,
        text,
        message: `Invalid JSON: ${error instanceof Error ? error.message : String(error)}`,
      };
      return;
    }
    const succeeded = await this.perform(
      "Set",
      async (session) => {
        const response = await session.set(path, text);
        return {
          failed: !response.ok,
          responseMs: response.responseMs,
          text: response.ok
            ? "Set succeeded"
            : response.kind === "publish"
              ? `Set: value may have changed — publication failed. ${response.message}`
              : `Set failed: ${response.message || response.code}`,
        };
      },
      path,
    );
    // An acknowledgement cannot discard edits made while the request was in flight.
    if (succeeded && this.data.selectedPath === path && this.editor === text) {
      this.revert();
      this.validation = undefined;
    }
  }

  async prune(): Promise<void> {
    if (!this.canPrune) return;
    this.result = undefined;
    await this.perform("Prune", async (session) => {
      const result = await session.prune();
      return {
        failed: result.error !== undefined,
        text:
          result.error === undefined
            ? `Cleared ${result.cleared}`
            : `Cleared ${result.cleared}; pruning interrupted, remaining outcome unknown. ${result.error}`,
      };
    });
  }

  private async perform(
    action: Action,
    operation: (session: CommandSession["session"]) => Promise<ActionResult>,
    path?: string,
  ): Promise<boolean> {
    const connection = this.getSession();
    if (!connection || this.pending.has(action)) return false;
    this.pending = new Set([...this.pending, action]);
    let result: ActionResult;
    try {
      result = await operation(connection.session);
    } catch (error) {
      result = {
        text: `${action}: ${error instanceof Error ? error.message : String(error)}`,
        failed: true,
      };
    }
    if (connection.signal.aborted) return false;
    this.pending = new Set([...this.pending].filter((item) => item !== action));
    // An unrelated success must not dismiss an unseen failure.
    if (!this.result?.failed || result.failed) this.result = result;
    this.log(
      action.toLowerCase(),
      (path === undefined
        ? result.text
        : `${displayPath(path)}: ${result.text}`) +
        (result.responseMs === undefined
          ? ""
          : ` · ${Math.round(result.responseMs)} ms`),
    );
    return !result.failed;
  }
}
