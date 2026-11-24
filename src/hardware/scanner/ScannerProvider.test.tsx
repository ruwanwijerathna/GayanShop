import { act, render } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ScannerProvider, useBillScanFallback, useScanHandler, useScanner } from "./ScannerProvider";

function Screen({ name, log }: { name: string; log: string[] }) {
  useScanHandler((code) => {
    log.push(`${name}:${code}`);
  });
  return null;
}

function Till({ log }: { log: string[] }) {
  useScanHandler((code) => void log.push(`till:${code}`), { handlesBills: true });
  return null;
}

function Elsewhere({ log }: { log: string[] }) {
  useBillScanFallback((code) => void log.push(`bill:${code}`));
  return null;
}

let capturing: (() => boolean) | null = null;
function Probe() {
  const scanner = useScanner();
  useEffect(() => {
    capturing = scanner.isCapturing;
  }, [scanner]);
  return null;
}

function scan(code: string) {
  for (const c of code) {
    window.dispatchEvent(
      new KeyboardEvent("keydown", { code: `Digit${c}`, key: c, bubbles: true, cancelable: true }),
    );
    vi.advanceTimersByTime(5);
  }
  window.dispatchEvent(
    new KeyboardEvent("keydown", { code: "Enter", key: "Enter", bubbles: true, cancelable: true }),
  );
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("ScannerProvider", () => {
  it("delivers scans in order to the screen in front, then back to the one below", async () => {
    const log: string[] = [];
    const { rerender } = render(
      <ScannerProvider>
        <Screen name="till" log={log} />
        <Screen name="dialog" log={log} />
      </ScannerProvider>,
    );
    await act(async () => {
      scan("1111111");
      scan("2222222");
      await vi.runAllTimersAsync();
    });
    expect(log).toEqual(["dialog:1111111", "dialog:2222222"]);

    rerender(
      <ScannerProvider>
        <Screen name="till" log={log} />
      </ScannerProvider>,
    );
    await act(async () => {
      scan("3333333");
      await vi.runAllTimersAsync();
    });
    expect(log.at(-1)).toBe("till:3333333");
  });

  it("reports an open capture so shortcuts can stay quiet", () => {
    render(
      <ScannerProvider>
        <Probe />
      </ScannerProvider>,
    );
    window.dispatchEvent(new KeyboardEvent("keydown", { code: "F13", key: "F13", cancelable: true }));
    expect(capturing?.()).toBe(true);
    window.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", key: "Enter", cancelable: true }));
    expect(capturing?.()).toBe(false);
  });

  it("stops listening when unmounted", async () => {
    const log: string[] = [];
    const { unmount } = render(
      <ScannerProvider>
        <Screen name="till" log={log} />
      </ScannerProvider>,
    );
    unmount();
    await act(async () => {
      scan("4444444");
      await vi.runAllTimersAsync();
    });
    expect(log).toEqual([]);
  });

  it("sends a scanned bill number to the fallback unless the screen in front handles bills", async () => {
    const log: string[] = [];
    const bill = (code: string) =>
      act(async () => {
        for (const c of code) {
          const key = c === "-" ? "Minus" : /\d/.test(c) ? `Digit${c}` : `Key${c}`;
          window.dispatchEvent(
            new KeyboardEvent("keydown", { code: key, key: c, bubbles: true, cancelable: true }),
          );
          vi.advanceTimersByTime(5);
        }
        window.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", key: "Enter", bubbles: true }));
        await vi.runAllTimersAsync();
      });
    const { rerender } = render(
      <ScannerProvider>
        <Elsewhere log={log} />
        <Screen name="products" log={log} />
      </ScannerProvider>,
    );
    await bill("T01-261121154");
    await bill("2000123456781");
    expect(log).toEqual(["bill:T01-261121154", "products:2000123456781"]);

    rerender(
      <ScannerProvider>
        <Elsewhere log={log} />
        <Till log={log} />
      </ScannerProvider>,
    );
    await bill("T01-261121154");
    expect(log.at(-1)).toBe("till:t01-261121154");
  });
});
