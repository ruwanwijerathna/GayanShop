// Money is always an integer count of minor units (LKR cents). Never a float. Mirrors
// src-tauri/src/domain/money.rs so previews match the receipt exactly.

export type Money = number;

const MAX_SAFE_CENTS = Number.MAX_SAFE_INTEGER;

function group(whole: string): string {
  let out = "";
  for (let i = 0; i < whole.length; i += 1) {
    if (i > 0 && (whole.length - i) % 3 === 0) out += ",";
    out += whole.charAt(i);
  }
  return out;
}

/** "Rs 1,234.50" — Latin digits in both UI languages (doc §1.8). */
export function formatMoney(cents: Money): string {
  if (!Number.isSafeInteger(cents)) throw new RangeError("money must be an integer number of cents");
  const sign = cents < 0 ? "-" : "";
  const abs = Math.abs(cents);
  const whole = Math.trunc(abs / 100).toString();
  const frac = (abs % 100).toString().padStart(2, "0");
  return `${sign}Rs ${group(whole)}.${frac}`;
}

/** Label price: "Rs. 1,250" for whole rupees, "Rs. 1,250.50" otherwise (doc §7.4). */
export function formatLabelPrice(cents: Money): string {
  const full = formatMoney(cents).replace("Rs ", "Rs. ");
  return full.endsWith(".00") ? full.slice(0, -3) : full;
}

/** Parse a typed major-unit amount ("450", "450.5", "1,250.75") into cents; null if invalid. */
export function parseMajor(input: string): Money | null {
  const s = input.trim().replaceAll(",", "");
  if (!/^\d*(\.\d{0,2})?$/.test(s) || s === "" || s === ".") return null;
  const [whole = "", frac = ""] = s.split(".");
  const wholeCents = whole === "" ? 0 : Number.parseInt(whole, 10) * 100;
  const fracCents = frac === "" ? 0 : Number.parseInt(frac.padEnd(2, "0"), 10);
  const total = wholeCents + fracCents;
  return Number.isSafeInteger(total) && total <= MAX_SAFE_CENTS ? total : null;
}

/** Cents as plain major-unit text for an input field ("1250.50"), the inverse of `parseMajor`. */
export function toMajorInput(cents: Money): string {
  if (!Number.isSafeInteger(cents)) throw new RangeError("money must be an integer number of cents");
  const sign = cents < 0 ? "-" : "";
  const abs = Math.abs(cents);
  return `${sign}${Math.trunc(abs / 100)}.${(abs % 100).toString().padStart(2, "0")}`;
}
