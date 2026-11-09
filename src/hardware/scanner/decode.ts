// Layout-independent key decoding (doc §7.1). A HID wedge "types" through the active Windows layout, so with
// a Sinhala layout `event.key` would be Sinhala text. We decode from `KeyboardEvent.code` (the physical key)
// and apply Shift and Caps Lock ourselves, as a US layout would.

const SHIFTED_DIGITS = ")!@#$%^&*(";

const PUNCTUATION: Record<string, [string, string]> = {
  Minus: ["-", "_"],
  Equal: ["=", "+"],
  BracketLeft: ["[", "{"],
  BracketRight: ["]", "}"],
  Backslash: ["\\", "|"],
  Semicolon: [";", ":"],
  Quote: ["'", '"'],
  Comma: [",", "<"],
  Period: [".", ">"],
  Slash: ["/", "?"],
  Backquote: ["`", "~"],
  Space: [" ", " "],
  NumpadDecimal: [".", "."],
  NumpadAdd: ["+", "+"],
  NumpadSubtract: ["-", "-"],
  NumpadMultiply: ["*", "*"],
  NumpadDivide: ["/", "/"],
};

export interface KeyLike {
  code: string;
  shiftKey: boolean;
  /** Caps Lock state (`getModifierState("CapsLock")`). */
  capsLock: boolean;
}

/** The printable character a physical key produces on a US layout, or `null` for non-printing keys. */
export function decodeKey({ code, shiftKey, capsLock }: KeyLike): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter?.[1]) {
    const upper = shiftKey !== capsLock;
    return upper ? letter[1] : letter[1].toLowerCase();
  }
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit?.[1]) return shiftKey ? (SHIFTED_DIGITS[Number.parseInt(digit[1], 10)] ?? null) : digit[1];
  const numpad = /^Numpad([0-9])$/.exec(code);
  if (numpad?.[1]) return numpad[1];
  const p = PUNCTUATION[code];
  if (p) return shiftKey ? p[1] : p[0];
  return null;
}
