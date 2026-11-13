/** Whole units: "12" → 12, "-3" → -3 when `signed`. Anything else is null. */
export function parseUnits(text: string, signed = false): number | null {
  const s = text.trim();
  if (!(signed ? /^-?\d{1,7}$/ : /^\d{1,7}$/).test(s)) return null;
  return Number.parseInt(s, 10);
}
