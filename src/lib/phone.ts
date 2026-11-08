// Sri Lankan phone numbers — preview-side mirror of src-tauri/src/domain/phone.rs (Rust is authoritative).

/** Normalise to E.164 (`+94` + 9 national digits), or null if not a Sri Lankan number. */
export function normalizeLk(input: string): string | null {
  const trimmed = input.trim();
  if (!/^\+?[\d\s()-]*$/.test(trimmed)) return null;
  const hadPlus = trimmed.startsWith("+");
  const digits = trimmed.replace(/\D/g, "");
  let national: string;
  if (digits.length === 11 && digits.startsWith("94")) national = digits.slice(2);
  else if (digits.length === 10 && !hadPlus && digits.startsWith("0")) national = digits.slice(1);
  else if (digits.length === 9 && !hadPlus) national = digits;
  else return null;
  return /^[1-9]\d{8}$/.test(national) ? `+94${national}` : null;
}

function national(e164: string): string | null {
  const n = e164.startsWith("+94") ? e164.slice(3) : "";
  return /^\d{9}$/.test(n) ? n : null;
}

/** Receipt mask, owner format `07X XXX X123`. */
export function maskForReceipt(e164: string): string | null {
  const n = national(e164);
  return n ? `0${n.charAt(0)}X XXX X${n.slice(6)}` : null;
}

/** Screen format `077 123 4567`. */
export function formatNational(e164: string): string | null {
  const n = national(e164);
  return n ? `0${n.slice(0, 2)} ${n.slice(2, 5)} ${n.slice(5)}` : null;
}
