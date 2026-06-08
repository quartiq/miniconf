const DEFAULT_LIMIT = 100;

export class EventLog {
  lines: string[] = [];

  constructor(
    private readonly sync: () => void,
    private readonly limit = DEFAULT_LIMIT,
  ) {}

  add(open: boolean, event: string, detail: string): void {
    if (!open) {
      return;
    }
    this.lines = [
      `${new Date().toLocaleTimeString(undefined)} ${event}: ${detail}`,
      ...this.lines,
    ].slice(0, this.limit);
    this.sync();
  }

  clearHidden(open: boolean): void {
    if (open || !this.lines.length) {
      return;
    }
    this.lines = [];
    this.sync();
  }

  dispose(): void {}
}
