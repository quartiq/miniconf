const DEFAULT_LIMIT = 100;
const DEFAULT_FLUSH_MS = 500;

export class EventLog {
  lines: string[] = [];
  private buffer: string[] = [];
  private timer: ReturnType<typeof globalThis.setTimeout> | undefined;

  constructor(
    private readonly sync: () => void,
    private readonly limit = DEFAULT_LIMIT,
    private readonly flushMs = DEFAULT_FLUSH_MS,
  ) {}

  add(open: boolean, event: string, detail: string): void {
    if (!open) {
      return;
    }
    this.buffer.unshift(`${new Date().toLocaleTimeString(undefined)} ${event}: ${detail}`);
    this.buffer.length = Math.min(this.buffer.length, this.limit);
    if (this.timer !== undefined) {
      return;
    }
    this.timer = globalThis.setTimeout(() => {
      this.timer = undefined;
      this.lines = [...this.buffer];
      this.sync();
    }, this.flushMs);
  }

  clearHidden(open: boolean): void {
    if (open || (!this.lines.length && !this.buffer.length && this.timer === undefined)) {
      return;
    }
    if (this.timer !== undefined) {
      globalThis.clearTimeout(this.timer);
      this.timer = undefined;
    }
    this.buffer.length = 0;
    this.lines = [];
  }

  dispose(): void {
    if (this.timer !== undefined) {
      globalThis.clearTimeout(this.timer);
      this.timer = undefined;
    }
  }
}
