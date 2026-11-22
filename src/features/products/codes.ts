// Client-side mirror of Rust's `domain::receipt_no` code formats, so the Generate buttons can fill the
// input at once: SKU `{device digit}{seq:05}` (6 digits, the cashier's typed shorthand, never printed) and the
// in-store barcode `2{device digit}{seq:05}{check}` (8 digits with an EAN-8 check digit, printed on labels as
// Code128). Rust still validates on save (uniqueness), so a clash surfaces as SKU_TAKEN.
import type { Product } from "@/db/bindings";

/** The highest sequence either generator hands out (Rust refuses beyond it: CODE_SEQUENCE_EXHAUSTED). */
const MAX_SEQ = 99_999;

/** T01 → 1; unknown or out-of-range device codes fall back to counter 1. */
export function deviceDigit(deviceCode: string | null | undefined): number {
  const n = Number.parseInt((deviceCode ?? "").replace(/\D/g, ""), 10);
  return n >= 1 && n <= 9 ? n : 1;
}

/** EAN-8 check digit for the first 7 digits (weights 3, 1, 3, … from the left). */
export function ean8CheckDigit(first7: string): number {
  let sum = 0;
  for (let i = 0; i < 7; i++) sum += (first7.charCodeAt(i) - 48) * (i % 2 === 0 ? 3 : 1);
  return (10 - (sum % 10)) % 10;
}

const skuSeq = (digit: number, code: string | null | undefined): number => {
  const m = code ? new RegExp(`^${digit}(\\d{5})$`).exec(code) : null;
  return m?.[1] ? Number.parseInt(m[1], 10) : 0;
};

/** The sequence inside an in-store barcode: the 8-digit format, or the 13-digit one generated before it. */
const barcodeSeq = (digit: number, code: string | null | undefined): number => {
  const m = code ? new RegExp(`^2${digit}(\\d{5}|\\d{10})\\d$`).exec(code) : null;
  return m?.[1] ? Number.parseInt(m[1], 10) : 0;
};

const usedSeq = (digit: number, products: readonly Product[]): number[] =>
  products.flatMap((p) => [skuSeq(digit, p.sku), barcodeSeq(digit, p.barcode)]);

/** The next free 6-digit SKU for this counter, above every SKU/barcode sequence already used. */
export function nextSku(digit: number, products: readonly Product[], typed: string): string {
  const used = Math.max(0, skuSeq(digit, typed.trim()), ...usedSeq(digit, products));
  return `${digit}${String(Math.min(used + 1, MAX_SEQ)).padStart(5, "0")}`;
}

/** The next free 8-digit in-store barcode for this counter. */
export function nextBarcode(digit: number, products: readonly Product[], typed: string): string {
  const used = Math.max(0, barcodeSeq(digit, typed.trim()), ...usedSeq(digit, products));
  const first7 = `2${digit}${String(Math.min(used + 1, MAX_SEQ)).padStart(5, "0")}`;
  return `${first7}${ean8CheckDigit(first7)}`;
}
