export class FlashSet {
  private serial = 0;
  private until = new Map<string, number>();

  constructor(private readonly onChange: (paths: Set<string>) => void) {}

  add(paths: Iterable<string>, duration = 1000): void {
    const serial = ++this.serial;
    const next = new Map(this.until);
    for (const path of paths) {
      next.set(path, serial);
    }
    this.until = next;
    this.onChange(new Set(this.until.keys()));
    globalThis.setTimeout(() => {
      const next = new Map(this.until);
      for (const [path, pathSerial] of next) {
        if (pathSerial <= serial) {
          next.delete(path);
        }
      }
      this.until = next;
      this.onChange(new Set(this.until.keys()));
    }, duration);
  }

  reset(): void {
    this.serial += 1;
    this.until = new Map();
    this.onChange(new Set());
  }
}
