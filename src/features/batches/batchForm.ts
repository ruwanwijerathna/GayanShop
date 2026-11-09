// Parsing for the batch forms: whole-unit quantities and rupee amounts → integer cents (Rust re-validates).
import { parseMajor } from "@/lib/money";

/** "12" → 12; anything that isn't a whole number ≥ 1 → null. */
export function parseQty(input: string): number | null {
  const s = input.trim();
  if (!/^\d{1,7}$/.test(s)) return null;
  const n = Number.parseInt(s, 10);
  return n >= 1 ? n : null;
}

/** Optional amount: "" → 0, otherwise rupees → cents, or null if it doesn't parse. */
export function parseOptionalMoney(input: string): number | null {
  return input.trim() === "" ? 0 : parseMajor(input);
}
