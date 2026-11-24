// Bill numbers (doc §6.4): `T01-261121004` = counter code, business date (YYMMDD), the day's bill number (3+ digits).
// Bills made before the short format keep `T01-20261121-0004`. Neither shape can be a product barcode (digits only),
// so a scanned code that matches is a bill of this shop.
const NEW_BILL = /^T0\d-\d{9,}$/i;
const OLD_BILL = /^T0\d-\d{8}-\d{4,}$/i;

export function isBillNumber(code: string): boolean {
  const c = code.trim();
  return NEW_BILL.test(c) || OLD_BILL.test(c);
}
