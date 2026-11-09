// Race-free HID-wedge capture (doc §7.1, §13.3). One `keydown` listener in the window CAPTURE phase sees every
// key before any input, button or shortcut handler does.
//
// - Prefix mode (deterministic): the scanner sends F13 (or Ctrl+Shift+F12) first, so capture opens before the
//   first character and nothing can leak anywhere. Every key up to the suffix Enter is swallowed.
// - Timing fallback (unconfigured scanners): keys ≤ 30 ms apart, ≥ 6 characters, ended by Enter. The first key
//   is let through (we can't know yet); from the second fast key on, keys are provisionally swallowed and
//   buffered. A scan then removes the one leaked character; a burst that turns out to be human typing (a gap
//   over 30 ms, a non-character key, or Enter too early) is replayed into the focused field with setRangeText.
// - The scan's Enter is always swallowed, so it can't submit a form or press a focused Pay button, and F-keys
//   are swallowed while capture is open, so no shortcut fires mid-scan.
// - IME composition is ignored entirely.
import { decodeKey } from "./decode";

export interface CaptureOptions {
  onScan: (code: string) => void;
  /** Clock in ms (tests inject a fake one). */
  now?: () => number;
  /** Largest gap between two scanner keys. */
  maxGap?: number;
  /** Shortest code the timing fallback accepts. */
  minLength?: number;
  /** Silence that abandons an unfinished prefixed capture. */
  prefixTimeout?: number;
}

type Editable = HTMLInputElement | HTMLTextAreaElement;

const MODIFIERS = new Set([
  "ShiftLeft",
  "ShiftRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "MetaLeft",
  "MetaRight",
  "CapsLock",
]);
const TEXT_INPUT_TYPES = new Set(["text", "search", "tel", "url", "email", "password", "number", ""]);

function editable(el: Element | null): Editable | null {
  if (el instanceof HTMLTextAreaElement) return el;
  if (el instanceof HTMLInputElement && TEXT_INPUT_TYPES.has(el.type)) return el;
  return null;
}

function isEnter(e: KeyboardEvent): boolean {
  return e.code === "Enter" || e.code === "NumpadEnter";
}

function isPrefix(e: KeyboardEvent): boolean {
  return e.code === "F13" || (e.code === "F12" && e.ctrlKey && e.shiftKey);
}

function swallow(e: KeyboardEvent) {
  e.preventDefault();
  e.stopPropagation();
  e.stopImmediatePropagation();
}

/** Insert or delete text at the caret the way typing would, so React's onChange sees it. */
function edit(el: Editable, text: string, start: number, end: number) {
  try {
    el.setRangeText(text, start, end, "end");
  } catch {
    return; // e.g. type="number", which has no selection API: nothing we can safely do
  }
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

export class ScannerCapture {
  private readonly opts: Required<CaptureOptions>;
  private mode: "idle" | "prefix" | "timing" = "idle";
  /** The decoded (US-layout) characters: the scan code. */
  private buf = "";
  /** What the active layout actually typed (`event.key`): replayed for a person, matched for the leak. */
  private typed = "";
  private last = 0;
  /** ≥ 2 fast keys seen: we are swallowing provisionally. */
  private provisional = false;
  /** Where the first (let-through) key went. */
  private target: Editable | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(opts: CaptureOptions) {
    this.opts = {
      now: () => performance.now(),
      maxGap: 30,
      minLength: 6,
      prefixTimeout: 300,
      ...opts,
    };
  }

  /** True while keys are being swallowed into a scan: shortcuts must stay quiet. */
  capturing(): boolean {
    return this.mode === "prefix" || (this.mode === "timing" && this.provisional);
  }

  readonly handleKeyDown = (e: KeyboardEvent): void => {
    if (e.isComposing || e.keyCode === 229) return;

    if (isPrefix(e)) {
      this.flushHuman();
      swallow(e);
      this.mode = "prefix";
      this.buf = "";
      this.arm(this.opts.prefixTimeout);
      return;
    }

    if (this.mode === "prefix") {
      swallow(e);
      if (isEnter(e)) {
        const code = this.buf;
        this.reset();
        if (code.length > 0) this.opts.onScan(code);
        return;
      }
      const ch = decodeKey({ code: e.code, shiftKey: e.shiftKey, capsLock: e.getModifierState("CapsLock") });
      if (ch !== null) this.buf += ch;
      this.arm(this.opts.prefixTimeout);
      return;
    }

    if (MODIFIERS.has(e.code)) return;

    const now = this.opts.now();
    if (this.mode === "timing" && now - this.last > this.opts.maxGap) this.flushHuman();

    if (isEnter(e)) {
      if (this.mode !== "timing") return;
      if (this.buf.length >= this.opts.minLength) {
        swallow(e);
        const code = this.buf;
        this.removeLeak();
        this.reset();
        this.opts.onScan(code);
      } else {
        this.flushHuman(); // too short to be a scan: the Enter is the user's
      }
      return;
    }

    if (this.capturing() && /^F\d+$/.test(e.code)) {
      swallow(e); // no shortcut mid-scan
      return;
    }

    const ch =
      e.ctrlKey || e.altKey || e.metaKey
        ? null
        : decodeKey({ code: e.code, shiftKey: e.shiftKey, capsLock: e.getModifierState("CapsLock") });
    if (ch === null) {
      this.flushHuman();
      return;
    }

    const typed = [...e.key].length === 1 ? e.key : ch;
    if (this.mode === "idle") {
      // The first key goes through untouched; it might be a person typing.
      this.mode = "timing";
      this.buf = ch;
      this.typed = typed;
      this.provisional = false;
      this.target = editable(document.activeElement);
    } else {
      swallow(e);
      this.buf += ch;
      this.typed += typed;
      this.provisional = true;
    }
    this.last = now;
    this.arm(this.opts.maxGap + 1);
  };

  dispose() {
    this.clearTimer();
  }

  private arm(ms: number) {
    this.clearTimer();
    this.timer = setTimeout(() => {
      this.timer = null;
      if (this.mode === "prefix") this.reset();
      else this.flushHuman();
    }, ms);
  }

  private clearTimer() {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  /** Not a scan after all: give the swallowed characters back to the field they were meant for. */
  private flushHuman() {
    if (this.mode === "timing" && this.provisional) {
      const el = editable(document.activeElement);
      if (el !== null && el === this.target) {
        const rest = [...this.typed].slice(1).join(""); // the first key already landed by itself
        edit(el, rest, el.selectionStart ?? el.value.length, el.selectionEnd ?? el.value.length);
      }
    }
    this.reset();
  }

  /** A scan: take back the one character that leaked into the focused field before we knew. */
  private removeLeak() {
    const el = this.target;
    if (el === null || el !== editable(document.activeElement)) return;
    const pos = el.selectionStart;
    if (pos === null || pos < 1) return;
    const leaked = [...this.typed][0] ?? "";
    const start = pos - leaked.length;
    if (leaked !== "" && start >= 0 && el.value.slice(start, pos) === leaked) edit(el, "", start, pos);
  }

  private reset() {
    this.clearTimer();
    this.mode = "idle";
    this.buf = "";
    this.typed = "";
    this.provisional = false;
    this.target = null;
  }
}
