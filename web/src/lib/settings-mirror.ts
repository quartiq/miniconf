// Coalesces authoritative /settings publications into the visible settings map.
// Absence is not inferred: empty settings payloads delete exact leaves, while
// reconnect/schema reload clears the map before retained replay.
export type Settings = Map<string, string>;

export type SettingsCommit = {
  settings: Settings;
  touched: Set<string>;
  activity: Set<string>;
  rev?: string;
};

export class SettingsMirror {
  private touched = new Set<string>();
  private rev: string | undefined;
  private shadow: Settings = new Map();
  private timer: ReturnType<typeof globalThis.setTimeout> | undefined;

  constructor(
    private readonly onCommit: (commit: SettingsCommit) => void,
    private readonly commitDelayMs = 100,
  ) {}

  clear(): void {
    this.cancel();
    const touched = new Set(this.shadow.keys());
    this.touched = new Set();
    this.rev = undefined;
    this.shadow = new Map();
    this.onCommit({ settings: new Map(), touched, activity: new Set() });
  }

  ingest(path: string, text: string | undefined, rev?: string): void {
    this.rev = rev ?? this.rev;
    if (text !== undefined) {
      this.shadow.set(path, text);
    } else {
      this.shadow.delete(path);
    }
    this.touched.add(path);
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
    const touched = new Set(this.touched);
    this.touched = new Set();
    this.onCommit({
      settings: new Map(this.shadow),
      touched,
      activity: touched,
      rev: this.rev,
    });
  }

  private cancel(): void {
    if (this.timer !== undefined) {
      globalThis.clearTimeout(this.timer);
      this.timer = undefined;
    }
  }
}
