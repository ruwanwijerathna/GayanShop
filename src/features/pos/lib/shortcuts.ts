// Till shortcut routing (doc §1.4, §1.9), as a pure function so every rule is unit-tested:
// - nothing fires while the scanner is capturing (§7.1);
// - nothing fires while a dialog is open (the dialog owns the keyboard; Radix handles its own Esc);
// - in a text field only F-keys and Esc fire, except in the scan field, where Numpad + and Numpad / do too
//   (they can't be part of a barcode, and focus always returns to the scan field);
// - Ctrl+L and Ctrl+Shift+L belong to the shell, not the till.

export type PosAction =
  | "help"
  | "quickSale"
  | "search"
  | "phone"
  | "lineDiscount"
  | "billDiscount"
  | "hold"
  | "pay"
  | "void"
  | "recall"
  | "cancel";

export interface KeyInput {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}

export interface ShortcutContext {
  capturing: boolean;
  dialogOpen: boolean;
  /** Focus is in a text input other than the scan field. */
  inTextField: boolean;
}

const F_KEYS: Record<string, PosAction> = {
  F1: "help",
  F2: "quickSale",
  F3: "search",
  F4: "phone",
  F6: "lineDiscount",
  F7: "billDiscount",
  F8: "hold",
  F9: "pay",
  F10: "void",
  F11: "recall",
};

const NUMPAD: Record<string, PosAction> = {
  NumpadAdd: "pay",
  NumpadDivide: "phone",
};

export function routeShortcut(e: KeyInput, ctx: ShortcutContext): PosAction | null {
  if (ctx.capturing || ctx.dialogOpen) return null;
  if (e.ctrlKey || e.altKey || e.metaKey) return null;
  const f = F_KEYS[e.code];
  if (f) return f;
  if (e.code === "Escape") return "cancel";
  if (ctx.inTextField) return null;
  return NUMPAD[e.code] ?? null;
}

/** The `<Kbd>` label for each action. */
export const SHORTCUT_LABELS: Record<PosAction, string> = {
  help: "F1",
  quickSale: "F2",
  search: "F3",
  phone: "F4",
  lineDiscount: "F6",
  billDiscount: "F7",
  hold: "F8",
  pay: "F9",
  void: "F10",
  recall: "F11",
  cancel: "Esc",
};
