// Ordered scan processing (doc §7.1): one worker drains a FIFO, so scans apply in the order they were made
// even when their lookups resolve out of order.

export type ScanHandler = (code: string) => Promise<void> | void;

export class ScanQueue {
  private readonly items: string[] = [];
  private running: Promise<void> | null = null;

  constructor(private readonly handle: ScanHandler) {}

  push(code: string) {
    this.items.push(code);
    // Start on a microtask: a drain that never awaits would otherwise finish (and clear `running`) before
    // this assignment, leaving a stale promise behind.
    this.running ??= Promise.resolve().then(() => this.drain());
  }

  /** Resolves once every queued scan has been handled. */
  async idle(): Promise<void> {
    while (this.running) await this.running;
  }

  private async drain() {
    try {
      for (let code = this.items.shift(); code !== undefined; code = this.items.shift()) {
        try {
          await this.handle(code);
        } catch {
          // One failed lookup must not stall the scans behind it; the handler reports its own errors.
        }
      }
    } finally {
      this.running = null;
    }
  }
}
