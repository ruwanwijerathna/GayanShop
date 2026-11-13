// Client-side mirror of Rust's `domain::receipt_no` code formats, so the Generate buttons can fill the
// input at once: SKU `{device digit}{seq:05}`, in-store EAN-13 `2{device digit}{seq:010}{check}`.
// Rust still validates on save (uniqueness, check digit), so a clash surfaces as SKU_TAKEN / BARCODE_TAKEN.
import type { Product } from "@/db/bindings";

/** T01 → 1; unknown or out-of-range device codes fall back to counter 1. */
export function deviceDigit(deviceCode: string | null | undefined): number {
  const n = Number.parseInt((deviceCode ?? "").replace(/\D/g, ""), 10);
  return n >= 1 && n <= 9 ? n : 1;
}

export function ean13CheckDigit(first12: string): number {
  let sum = 0;
  for (let i = 0; i < 12; i++) sum += (first12.charCodeAt(i) - 48) * (i % 2 === 0 ? 1 : 3);
  return (10 - (sum % 10)) % 10;
}

const skuSeq = (digit: number, code: string | null | undefined): number => {
  const m = code ? new RegExp(`^${digit}(\\d{5})$`).exec(code) : null;
  return m?.[1] ? Number.parseInt(m[1], 10) : 0;
};

const barcodeSeq = (digit: number, code: string | null | undefined): number => {
  const m = code ? new RegExp(`^2${digit}(\\d{10})\\d$`).exec(code) : null;
  return m?.[1] ? Number.parseInt(m[1], 10) : 0;
};

/** The next free 6-digit SKU for this counter, above every SKU/barcode sequence already used. */
export function nextSku(digit: number, products: readonly Product[], typed: string): string {
  const used = Math.max(
    0,
    skuSeq(digit, typed.trim()),
    ...products.flatMap((p) => [skuSeq(digit, p.sku), barcodeSeq(digit, p.barcode)]),
  );
  return `${digit}${String(Math.min(used + 1, 99_999)).padStart(5, "0")}`;
}

/** The next free in-store EAN-13 for this counter. */
export function nextBarcode(digit: number, products: readonly Product[], typed: string): string {
  const used = Math.max(
    0,
    barcodeSeq(digit, typed.trim()),
    ...products.flatMap((p) => [skuSeq(digit, p.sku), barcodeSeq(digit, p.barcode)]),
  );
  const first12 = `2${digit}${String(used + 1).padStart(10, "0")}`;
  return `${first12}${ean13CheckDigit(first12)}`;
}
