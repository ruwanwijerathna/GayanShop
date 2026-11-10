// Block 2.10 — Phase 2 acceptance (doc §8 Phase 2 exit), scripted against the real app on the §13.4 gate
// dataset (50,000 products, 100,000 order items over a year at three counters; see src-tauri/tests/
// e2e_dataset.rs). It proves in the real WebView2 that the virtualised tables scroll at 60 fps at that scale,
// then walks the Phase 2 back office: the dashboard, order history down to a receipt, the yearly P&L with
// its charts, a customer's history, promoting a frequent quick sale, an expense, and rebuilding the reports.
// Printing and scanning real labels need the hardware: docs/qa/phase2-acceptance.md covers them.
import { $, browser, expect } from "@wdio/globals";
import { click, login, seeText, type } from "./helpers";

const OWNER_PIN = "246810";

interface Frames {
  /** The display's own frame interval, measured idle before scrolling (≈ 16.7 ms on a 60 Hz screen). */
  idle: number;
  frames: number;
  median: number;
  p95: number;
  scrollTop: number;
  scrollHeight: number;
}

/**
 * Scroll a DataTable by `step` px every animation frame for `ms`, recording each frame's duration: what a
 * person sees when they fling the list. Runs inside the page, so WebDriver's round trips don't count.
 */
async function scrollFrames(grid: string, step: number, ms: number): Promise<Frames> {
  return browser.execute(
    async (label: string, px: number, duration: number) => {
      const root = document.querySelector(`[role="grid"][aria-label="${label}"]`);
      const viewport = root?.querySelector<HTMLElement>(".overflow-auto");
      if (!viewport) throw new Error(`no grid "${label}"`);
      const median = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)] ?? Infinity;
      const idleFrames: number[] = [];
      await new Promise<void>((done) => {
        let last = performance.now();
        const tick = (t: number) => {
          idleFrames.push(t - last);
          last = t;
          if (idleFrames.length < 61) requestAnimationFrame(tick);
          else done();
        };
        requestAnimationFrame(tick);
      });
      idleFrames.shift();
      const frames: number[] = [];
      await new Promise<void>((done) => {
        let last = performance.now();
        const end = last + duration;
        const tick = (t: number) => {
          frames.push(t - last);
          last = t;
          viewport.scrollTop += px;
          if (t < end) requestAnimationFrame(tick);
          else done();
        };
        requestAnimationFrame(tick);
      });
      frames.shift();
      const sorted = [...frames].sort((a, b) => a - b);
      return {
        idle: median(idleFrames),
        frames: frames.length,
        median: sorted[Math.floor(sorted.length / 2)] ?? Infinity,
        p95: sorted[Math.floor(sorted.length * 0.95)] ?? Infinity,
        scrollTop: viewport.scrollTop,
        scrollHeight: viewport.scrollHeight,
      };
    },
    grid,
    step,
    ms,
  );
}

/**
 * Smooth = scrolling keeps the display's own frame rate (60 fps on a 60 Hz screen): the median frame is no
 * longer than an idle frame (+10 % timer jitter), and even the worst 5 % drop at most one frame.
 */
function expectSmooth(name: string, f: Frames) {
  process.stdout.write(
    `${name}: display frame ${Math.round(f.idle * 10) / 10} ms; ${f.frames} frames, median ${Math.round(f.median * 10) / 10} ms, p95 ${Math.round(f.p95 * 10) / 10} ms, scrolled ${f.scrollTop}px\n`,
  );
  expect(f.frames).toBeGreaterThan(100);
  expect(f.idle).toBeLessThanOrEqual(34);
  expect(f.median).toBeLessThanOrEqual(f.idle * 1.1);
  expect(f.p95).toBeLessThanOrEqual(f.idle * 2);
}

async function nav(to: string) {
  await click(to);
}

async function grid(name: string) {
  const el = await $(`[role="grid"][aria-label="${name}"]`);
  await el.waitForExist();
  return el;
}

describe("Phase 2 acceptance: the back office at full scale", () => {
  it("logs in and shows today's dashboard", async () => {
    await login("Gayan", OWNER_PIN);
    await seeText("Today");
    await $(`figure[aria-label="Sales trend"]`).waitForExist();
    await $(`figure[aria-label="Payment methods"]`).waitForExist();
  });

  it("scrolls 50,000 products at 60 fps", async () => {
    await nav("Products");
    const g = await grid("Products");
    await g.$('[role="row"][data-row-index]').waitForExist({ timeout: 30_000 });
    const f = await scrollFrames("Products", 240, 4_000);
    expect(f.scrollHeight).toBeGreaterThan(50_000 * 36);
    expectSmooth("products", f);
  });

  it("scrolls the year's orders at 60 fps while pages load, and opens a receipt", async () => {
    await nav("Orders");
    await click("All time");
    const g = await grid("Orders");
    await g.$('[role="row"][data-row-index]').waitForExist({ timeout: 30_000 });
    const f = await scrollFrames("Orders", 96, 4_000);
    expectSmooth("orders", f);
    expect(f.scrollHeight).toBeGreaterThan(200 * 48);
    await browser.execute(() => {
      const viewport = document.querySelector('[role="grid"][aria-label="Orders"] .overflow-auto');
      if (viewport) viewport.scrollTop = 0;
    });
    const first = await g.$('[role="row"][data-row-index="0"]');
    await first.waitForClickable();
    await first.click();
    await seeText("Receipt S0");
    await seeText("Total paid");
    await click("Close");
  });

  it("reports the year with its charts", async () => {
    await nav("Reports");
    await click("Year");
    await $(`table[aria-label="Profit and loss"]`).waitForExist({ timeout: 30_000 });
    await seeText("Net sales (revenue)");
    await seeText("Net profit");
    await $(`figure[aria-label="Sales trend"]`).waitForExist({ timeout: 30_000 });
    await $(`figure[aria-label="Revenue and expenses"]`).waitForExist();
    await $(`figure[aria-label="Batch sell-through"]`).waitForExist();
  });

  it("finds a customer by phone and shows their history", async () => {
    await nav("Customers");
    await type("Search by phone or name", "0710000042");
    const g = await grid("Customers");
    const row = await g.$(`.//*[normalize-space()="Customer 42"]`);
    await row.waitForClickable({ timeout: 30_000 });
    await row.click();
    const profile = await $(`section[aria-label="Customer profile"]`);
    await profile.waitForExist();
    await seeText("Purchase history");
  });

  it("promotes a frequent quick sale to a product", async () => {
    await nav("Unlisted Items");
    const item = await $(`//button[starts-with(normalize-space(), "Quick item")]`);
    await item.waitForClickable({ timeout: 30_000 });
    const name = await item.getText();
    await item.click();
    await type("Cost per unit (Rs)", "120");
    await click("Promote");
    await seeText(`${name} is now a product.`);
  });

  it("records an expense", async () => {
    await nav("Expenses");
    await click("Add expense");
    await type("Amount (Rs)", "1500");
    await type("Paid to (optional)", "Tea stall");
    await click("Save");
    await seeText("Expense recorded.");
    await seeText("Tea stall");
  });

  it("rebuilds the reports from the base tables", async () => {
    await nav("Settings");
    await click("Rebuild reports");
    await click("Rebuild now");
    await seeText("Reports rebuilt.");
    await seeText("day totals");
  });
});
