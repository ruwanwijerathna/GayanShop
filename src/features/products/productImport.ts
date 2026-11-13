// CSV import for products: parse + validate in the webview, then each valid row goes through the same
// typed commands as the Add product form. Money stays integer minor units via `parseMajor`.
import type { ProductInput } from "@/db/bindings";
import { parseMajor } from "@/lib/money";

export const IMPORT_COLUMNS = ["name", "price", "cost", "sku", "barcode", "quantity"] as const;
export type ImportColumn = (typeof IMPORT_COLUMNS)[number];

export const IMPORT_TEMPLATE = `${IMPORT_COLUMNS.join(",")}\nSunlight Soap 100g,150.00,110.00,,,50\n`;

export type ImportIssue = "nameMissing" | "priceInvalid" | "costInvalid" | "quantityInvalid";

export type ImportRow = {
  line: number;
  input: ProductInput | null;
  quantity: number;
  name: string;
  issue: ImportIssue | null;
};

/** RFC 4180-ish: quoted fields, doubled quotes, CRLF or LF. Blank lines are skipped. */
export function parseCsv(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  const src = text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
  for (let i = 0; i < src.length; i++) {
    const ch = src.charAt(i);
    if (quoted) {
      if (ch === '"' && src.charAt(i + 1) === '"') {
        field += '"';
        i++;
      } else if (ch === '"') quoted = false;
      else field += ch;
    } else if (ch === '"') quoted = true;
    else if (ch === ",") {
      row.push(field);
      field = "";
    } else if (ch === "\n" || ch === "\r") {
      if (ch === "\r" && src.charAt(i + 1) === "\n") i++;
      row.push(field);
      field = "";
      if (row.some((c) => c.trim() !== "")) rows.push(row);
      row = [];
    } else field += ch;
  }
  row.push(field);
  if (row.some((c) => c.trim() !== "")) rows.push(row);
  return rows;
}

/** Header row (any order, case-insensitive) → one validated `ImportRow` per data line. */
export function buildImport(text: string, canViewCost: boolean): ImportRow[] | "noHeader" {
  const [head, ...body] = parseCsv(text);
  if (!head) return "noHeader";
  const index = new Map<string, number>();
  head.forEach((h, i) => index.set(h.trim().toLowerCase(), i));
  if (!index.has("name") || !index.has("price")) return "noHeader";
  const cell = (r: string[], c: ImportColumn) => (r[index.get(c) ?? -1] ?? "").trim();

  return body.map((r, n): ImportRow => {
    const line = n + 2;
    const name = cell(r, "name");
    const fail = (issue: ImportIssue): ImportRow => ({ line, input: null, quantity: 0, name, issue });
    if (name === "") return fail("nameMissing");
    const price = parseMajor(cell(r, "price"));
    if (price === null) return fail("priceInvalid");
    let cost: number | null = null;
    if (canViewCost && cell(r, "cost") !== "") {
      cost = parseMajor(cell(r, "cost"));
      if (cost === null) return fail("costInvalid");
    }
    const qtyText = cell(r, "quantity");
    if (qtyText !== "" && !/^\d{1,7}$/.test(qtyText)) return fail("quantityInvalid");
    const sku = cell(r, "sku");
    const barcode = cell(r, "barcode");
    return {
      line,
      name,
      quantity: qtyText === "" ? 0 : Number.parseInt(qtyText, 10),
      issue: null,
      input: {
        kind: "standard",
        name,
        name_si: null,
        sku: sku === "" ? { mode: "none" } : { mode: "manual", value: sku },
        barcode: barcode === "" ? { mode: "none" } : { mode: "manual", value: barcode },
        category_id: null,
        default_cost: cost,
        price,
        min_price: null,
        track_stock: true,
        allow_negative_stock: true,
        attributes: {},
        is_active: true,
      },
    };
  });
}
