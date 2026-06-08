// Coalesces authoritative /settings publications into the visible settings map.
// Absence is not inferred: empty settings payloads delete exact leaves, while
// reconnect/schema reload clears the map before retained replay.
export type Settings = Map<string, unknown>;

export type SettingsCommit = {
  settings: Settings;
  changed: Set<string>;
  rev?: string;
};

export class SettingsMirror {
  private changed = new Set<string>();
  private rev: string | undefined;
  private shadow: Settings = new Map();
  private timer: ReturnType<typeof globalThis.setTimeout> | undefined;

  constructor(
    private readonly onCommit: (commit: SettingsCommit) => void,
    private readonly commitDelayMs = 100,
  ) {}

  reset(): void {
    this.cancel();
    this.changed = new Set();
    this.rev = undefined;
    this.shadow = new Map();
  }

  clear(): void {
    this.cancel();
    const changed = new Set(this.shadow.keys());
    this.changed = new Set();
    this.rev = undefined;
    this.shadow = new Map();
    if (changed.size) {
      this.onCommit({ settings: new Map(), changed });
    }
  }

  ingest(path: string, value: unknown, present: boolean, rev?: string): void {
    this.rev = rev ?? this.rev;
    if (present) {
      this.shadow.set(path, value);
    } else {
      this.shadow.delete(path);
    }
    this.changed.add(path);
    this.schedule();
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
      this.commit();
    }, this.commitDelayMs);
  }

  private commit(): void {
    const changed = new Set(this.changed);
    this.changed = new Set();
    this.onCommit({ settings: new Map(this.shadow), changed, rev: this.rev });
  }

  private cancel(): void {
    if (this.timer !== undefined) {
      globalThis.clearTimeout(this.timer);
      this.timer = undefined;
    }
  }
}
