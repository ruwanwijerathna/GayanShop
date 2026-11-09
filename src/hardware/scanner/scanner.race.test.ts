// Block 1.8 — scanner races (doc §7.1, §13.3): bursts at 5, 15 and 29 ms, human typing at 80 ms, a Sinhala
// layout, focus in an input, an Enter over the Pay button, the F13 prefix, F-keys mid-scan, IME composition,
// and the FIFO queue with out-of-order lookups.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ScannerCapture } from "./capture";
import { decodeKey } from "./decode";
import { ScanQueue } from "./queue";

/** US-layout `event.key` for a code (what Windows would report with an English layout). */
function usKey(code: string, shift = false): string {
  return decodeKey({ code, shiftKey: shift, capsLock: false }) ?? code;
}

const SINHALA_DIGITS: Record<string, string> = {
  Digit0: "෦",
  Digit1: "෧",
  Digit2: "෨",
  Digit3: "෩",
  Digit4: "෪",
  Digit5: "෫",
  Digit6: "෬",
  Digit7: "෭",
  Digit8: "෮",
  Digit9: "෯",
};

interface Press {
  code: string;
  key?: string;
  shift?: boolean;
  ctrl?: boolean;
  composing?: boolean;
}

/**
 * Dispatch a keydown where the focus is, then emulate the browser's default action (jsdom has none): a
 * printable key types into a focused text field, Enter clicks a focused button or submits the form.
 */
function press(p: Press): KeyboardEvent {
  const key = p.key ?? usKey(p.code, p.shift);
  const target = document.activeElement ?? document.body;
  const e = new KeyboardEvent("keydown", {
    code: p.code,
    key,
    shiftKey: p.shift ?? false,
    ctrlKey: p.ctrl ?? false,
    isComposing: p.composing ?? false,
    bubbles: true,
    cancelable: true,
  });
  target.dispatchEvent(e);
  if (e.defaultPrevented) return e;
  if (key === "Enter") {
    if (target instanceof HTMLButtonElement) target.click();
    else if (target instanceof HTMLInputElement) target.form?.requestSubmit();
  } else if ([...key].length === 1 && target instanceof HTMLInputElement) {
    const at = target.selectionStart ?? target.value.length;
    target.setRangeText(key, at, target.selectionEnd ?? at, "end");
    target.dispatchEvent(new Event("input", { bubbles: true }));
  }
  return e;
}

const DIGIT = (c: string) => `Digit${c}`;

/** A scanner burst: one key every `gap` ms, then Enter. */
function burst(code: string, gap: number, opts: { enter?: boolean; layout?: "us" | "si" } = {}) {
  for (const c of code) {
    press({ code: DIGIT(c), key: opts.layout === "si" ? SINHALA_DIGITS[DIGIT(c)] : c });
    vi.advanceTimersByTime(gap);
  }
  if (opts.enter ?? true) press({ code: "Enter", key: "Enter" });
}

/** A person typing: one key every `gap` ms, letting the fallback's flush timer run between keys. */
function type(text: string, gap: number) {
  for (const ch of text) {
    const code = /\d/.test(ch) ? DIGIT(ch) : `Key${ch.toUpperCase()}`;
    press({ code, key: ch, shift: ch !== ch.toLowerCase() });
    vi.advanceTimersByTime(gap);
  }
}

let scans: string[];
let capture: ScannerCapture;
let shortcuts: string[];
const onShortcut = (e: KeyboardEvent) => {
  if (/^F\d+$/.test(e.code)) shortcuts.push(e.code);
};

function mountInput(): HTMLInputElement {
  const form = document.createElement("form");
  const input = document.createElement("input");
  input.type = "text";
  form.append(input);
  document.body.append(form);
  input.focus();
  return input;
}

beforeEach(() => {
  vi.useFakeTimers();
  scans = [];
  shortcuts = [];
  capture = new ScannerCapture({ onScan: (c) => scans.push(c), now: () => Date.now() });
  window.addEventListener("keydown", capture.handleKeyDown, { capture: true });
  window.addEventListener("keydown", onShortcut);
});

afterEach(() => {
  window.removeEventListener("keydown", capture.handleKeyDown, { capture: true });
  window.removeEventListener("keydown", onShortcut);
  capture.dispose();
  document.body.innerHTML = "";
  vi.useRealTimers();
});

describe("decodeKey", () => {
  it("decodes from the physical key, applying Shift and Caps Lock itself", () => {
    expect(decodeKey({ code: "Digit7", shiftKey: false, capsLock: false })).toBe("7");
    expect(decodeKey({ code: "KeyA", shiftKey: false, capsLock: false })).toBe("a");
    expect(decodeKey({ code: "KeyA", shiftKey: true, capsLock: false })).toBe("A");
    expect(decodeKey({ code: "KeyA", shiftKey: false, capsLock: true })).toBe("A");
    expect(decodeKey({ code: "KeyA", shiftKey: true, capsLock: true })).toBe("a");
    expect(decodeKey({ code: "Minus", shiftKey: false, capsLock: false })).toBe("-");
    expect(decodeKey({ code: "Numpad4", shiftKey: false, capsLock: false })).toBe("4");
    expect(decodeKey({ code: "F2", shiftKey: false, capsLock: false })).toBeNull();
  });
});

describe("timing fallback", () => {
  it.each([5, 15, 29])("captures a burst at %i ms per key", (gap) => {
    burst("4791234567890", gap);
    expect(scans).toEqual(["4791234567890"]);
  });

  it("leaves human typing at 80 ms alone", () => {
    const input = mountInput();
    type("Mug12345", 80);
    press({ code: "Enter", key: "Enter" });
    expect(input.value).toBe("Mug12345");
    expect(scans).toEqual([]);
  });

  it("removes the one leaked character from a focused input", () => {
    const input = mountInput();
    input.value = "note ";
    input.setSelectionRange(5, 5);
    burst("2000123456781", 10);
    expect(scans).toEqual(["2000123456781"]);
    expect(input.value).toBe("note ");
  });

  it("decodes correctly under a Sinhala keyboard layout", () => {
    const input = mountInput();
    burst("4791234567890", 8, { layout: "si" });
    expect(scans).toEqual(["4791234567890"]);
    expect(input.value, "the leaked Sinhala character is removed too").toBe("");
  });

  it("replays a fast burst without Enter into the input (it was a person)", () => {
    const input = mountInput();
    burst("123456", 10, { enter: false });
    vi.advanceTimersByTime(50);
    expect(scans).toEqual([]);
    expect(input.value).toBe("123456");
  });

  it("replays a burst that pauses longer than 30 ms", () => {
    const input = mountInput();
    burst("123", 10, { enter: false });
    vi.advanceTimersByTime(40);
    type("45", 80);
    expect(input.value).toBe("12345");
    expect(scans).toEqual([]);
  });

  it("treats a short fast burst plus Enter as typing and lets that Enter through", () => {
    const input = mountInput();
    const submitted = vi.fn((e: Event) => e.preventDefault());
    input.form?.addEventListener("submit", submitted);
    burst("1234", 5);
    expect(scans).toEqual([]);
    expect(input.value).toBe("1234");
    expect(submitted).toHaveBeenCalledTimes(1);
  });

  it("replays in a Sinhala layout what the person actually typed", () => {
    const input = mountInput();
    burst("12345", 10, { enter: false, layout: "si" });
    vi.advanceTimersByTime(50);
    expect(input.value).toBe("෧෨෩෪෫");
  });
});

describe("the scan's Enter", () => {
  it("never presses a focused Pay button", () => {
    const pay = document.createElement("button");
    const paid = vi.fn();
    pay.addEventListener("click", paid);
    document.body.append(pay);
    pay.focus();
    burst("4791234567890", 10);
    expect(scans).toEqual(["4791234567890"]);
    expect(paid).not.toHaveBeenCalled();
  });

  it("never submits the form of a focused input", () => {
    const input = mountInput();
    const submitted = vi.fn((e: Event) => e.preventDefault());
    input.form?.addEventListener("submit", submitted);
    burst("4791234567890", 10);
    expect(scans).toEqual(["4791234567890"]);
    expect(submitted).not.toHaveBeenCalled();
  });

  it("is swallowed before any other listener sees it", () => {
    const seen: string[] = [];
    const later = (e: KeyboardEvent) => seen.push(e.code);
    document.addEventListener("keydown", later, { capture: true });
    burst("4791234567890", 10);
    document.removeEventListener("keydown", later, { capture: true });
    expect(seen).toEqual(["Digit4"]); // only the first key, before we could know
  });
});

describe("F13 prefix", () => {
  it("captures deterministically: nothing leaks, even slow keys", () => {
    const input = mountInput();
    press({ code: "F13", key: "F13" });
    for (const c of "AB-12") {
      const code = c === "-" ? "Minus" : /\d/.test(c) ? DIGIT(c) : `Key${c}`;
      press({ code, key: c, shift: /[A-Z]/.test(c) });
      vi.advanceTimersByTime(60); // slower than the timing fallback allows
    }
    press({ code: "Enter", key: "Enter" });
    expect(scans).toEqual(["AB-12"]);
    expect(input.value).toBe("");
    expect(shortcuts).toEqual([]);
  });

  it("accepts Ctrl+Shift+F12 for scanners that can't send F13", () => {
    press({ code: "F12", key: "F12", ctrl: true, shift: true });
    burst("123", 5);
    expect(scans).toEqual(["123"]);
  });

  it("abandons an unfinished prefixed capture after silence", () => {
    const input = mountInput();
    press({ code: "F13", key: "F13" });
    press({ code: "Digit1", key: "1" });
    vi.advanceTimersByTime(400);
    expect(capture.capturing()).toBe(false);
    type("9", 80);
    expect(input.value).toBe("9");
    expect(scans).toEqual([]);
  });
});

describe("shortcuts and IME", () => {
  it("suspends F-key shortcuts while a capture is open", () => {
    press({ code: "F13", key: "F13" });
    press({ code: "Digit1", key: "1" });
    expect(capture.capturing()).toBe(true);
    press({ code: "F9", key: "F9" });
    press({ code: "Enter", key: "Enter" });
    expect(shortcuts).toEqual([]);
    expect(scans).toEqual(["1"]);
    press({ code: "F9", key: "F9" });
    expect(shortcuts, "and only then").toEqual(["F9"]);
  });

  it("suspends F-keys mid-burst in the timing fallback too", () => {
    press({ code: "Digit4", key: "4" });
    vi.advanceTimersByTime(5);
    press({ code: "Digit7", key: "7" });
    press({ code: "F2", key: "F2" });
    expect(shortcuts).toEqual([]);
  });

  it("ignores IME composition entirely", () => {
    const input = mountInput();
    for (let i = 0; i < 8; i++) {
      const e = press({ code: "KeyA", key: "a", composing: true });
      expect(e.defaultPrevented).toBe(false);
      vi.advanceTimersByTime(5);
    }
    press({ code: "Enter", key: "Enter", composing: true });
    expect(scans).toEqual([]);
    expect(input.value).toBe("aaaaaaaa");
  });
});

describe("ScanQueue", () => {
  it("applies scans in scan order even when lookups resolve out of order", async () => {
    const applied: string[] = [];
    const delays: Record<string, number> = { a: 30, b: 1, c: 10 };
    const q = new ScanQueue(async (code) => {
      await new Promise((r) => setTimeout(r, delays[code]));
      applied.push(code);
    });
    q.push("a");
    q.push("b");
    q.push("c");
    const done = q.idle();
    await vi.advanceTimersByTimeAsync(100);
    await done;
    expect(applied).toEqual(["a", "b", "c"]);
  });

  it("keeps going after a failed lookup", async () => {
    const applied: string[] = [];
    const q = new ScanQueue((code) => {
      if (code === "bad") throw new Error("lookup failed");
      applied.push(code);
    });
    q.push("bad");
    q.push("ok");
    await q.idle();
    expect(applied).toEqual(["ok"]);
  });

  it("feeds two fast scans of the same code into one counter without losing either", async () => {
    let qty = 0;
    const q = new ScanQueue(async () => {
      await Promise.resolve();
      qty = qty + 1; // the functional `qty + 1` update the cart store applies
    });
    q.push("4791234567890");
    q.push("4791234567890");
    await q.idle();
    expect(qty).toBe(2);
  });
});
