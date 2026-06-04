// Merges retained scans and live /settings publications into the authoritative
// settings map. Retained scans use a shadow map so deletions are only committed
// after the scan completes successfully.
export type Settings = Map<string, unknown>;

export type SettingsCommit = {
  settings: Settings;
  changed: Set<string>;
  rev?: string;
  source: "live" | "retained";
};

export class SettingsMirror {
  private changed = new Set<string>();
  private loadingRetained = false;
  private retainedBase = new Set<string>();
  private retainedShadow: Settings | undefined;
  private rev: string | undefined;
  private shadow: Settings = new Map();
  private timer: ReturnType<typeof globalThis.setTimeout> | undefined;

  constructor(
    private readonly onCommit: (commit: SettingsCommit) => void,
    private readonly liveDelay = 100,
  ) {}

  reset(): void {
    this.cancel();
    this.changed = new Set();
    this.loadingRetained = false;
    this.retainedBase = new Set();
    this.retainedShadow = undefined;
    this.rev = undefined;
    this.shadow = new Map();
  }

  beginRetained(): void {
    this.cancel();
    this.changed = new Set();
    this.loadingRetained = true;
    this.retainedBase = new Set(this.shadow.keys());
    this.retainedShadow = new Map();
    this.rev = undefined;
  }

  ingest(path: string, value: unknown, present: boolean, rev?: string): void {
    this.rev = rev ?? this.rev;
    const shadow = this.retainedShadow ?? this.shadow;
    if (present) {
      shadow.set(path, value);
    } else {
      shadow.delete(path);
    }
    this.changed.add(path);
    if (!this.loadingRetained) {
      this.schedule();
    }
  }

  finishRetained(retained: Settings): void {
    const shadow = this.retainedShadow ?? new Map();
    for (const [path, value] of retained) {
      shadow.set(path, value);
      this.changed.add(path);
    }
    for (const path of this.retainedBase) {
      if (!shadow.has(path)) {
        this.changed.add(path);
      }
    }
    this.shadow = shadow;
    this.retainedBase = new Set();
    this.retainedShadow = undefined;
    this.loadingRetained = false;
    this.commit("retained");
  }

  failRetained(): void {
    this.changed = new Set();
    this.retainedBase = new Set();
    this.retainedShadow = undefined;
    this.loadingRetained = false;
  }

  dispose(): void {
    this.cancel();
  }

  private schedule(): void {
    if (this.timer !== undefined) {
      return;
    }
    this.timer = globalThis.setTimeout(() => {
      this.timer = undefined;
      this.commit("live");
    }, this.liveDelay);
  }

  private commit(source: SettingsCommit["source"]): void {
    const changed = new Set(this.changed);
    this.changed = new Set();
    this.onCommit({ settings: new Map(this.shadow), changed, rev: this.rev, source });
  }

  private cancel(): void {
    if (this.timer !== undefined) {
      globalThis.clearTimeout(this.timer);
      this.timer = undefined;
    }
  }
}
