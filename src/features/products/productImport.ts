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
  categoryName?: string;
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

  const getCell = (r: string[], ...aliases: string[]): string => {
    for (const alias of aliases) {
      const idx = index.get(alias.toLowerCase());
      if (idx !== undefined && r[idx] !== undefined) {
        const val = r[idx].trim();
        if (val !== "") return val;
      }
    }
    return "";
  };

  return body.map((r, n): ImportRow => {
    const line = n + 2;
    const name = getCell(r, "name");
    const fail = (issue: ImportIssue): ImportRow => ({ line, input: null, quantity: 0, name, issue });
    if (name === "") return fail("nameMissing");
    const price = parseMajor(getCell(r, "price"));
    if (price === null) return fail("priceInvalid");
    let cost: number | null = null;
    const costText = getCell(r, "default_cost", "cost");
    if (canViewCost && costText !== "") {
      cost = parseMajor(costText);
      if (cost === null) return fail("costInvalid");
    }
    const qtyText = getCell(r, "on_hand", "quantity", "stock", "qty");
    if (qtyText !== "" && !/^-?\d{1,7}$/.test(qtyText)) return fail("quantityInvalid");
    const sku = getCell(r, "sku");
    const barcode = getCell(r, "barcode");
    const categoryName = getCell(r, "category", "category_name");
    const nameSi = getCell(r, "name_si", "sinhala_name");
    const trackStockVal = getCell(r, "track_stock");
    const trackStock =
      trackStockVal === "" ? true : !["no", "false", "0"].includes(trackStockVal.toLowerCase());
    const activeVal = getCell(r, "active", "is_active");
    const isActive = activeVal === "" ? true : !["no", "false", "0"].includes(activeVal.toLowerCase());

    const quantity = qtyText === "" ? 0 : Math.max(0, Number.parseInt(qtyText, 10));

    return {
      line,
      name,
      categoryName: categoryName || undefined,
      quantity,
      issue: null,
      input: {
        kind: "standard",
        name,
        name_si: nameSi || null,
        sku: sku === "" ? { mode: "none" } : { mode: "manual", value: sku },
        barcode: barcode === "" ? { mode: "none" } : { mode: "manual", value: barcode },
        category_id: null,
        default_cost: cost,
        price,
        min_price: null,
        track_stock: trackStock,
        allow_negative_stock: true,
        attributes: {},
        is_active: isActive,
      },
    };
  });
}
