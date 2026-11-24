import { Download, FileUp } from "lucide-react";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "@/app/session";
import { Button } from "@/components/ui/button";
import type { Product_Serialize as Product } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useAdjustStock, useCategories, useProducts, useSaveCategory, useSaveProduct } from "@/db/hooks";
import { translateError } from "@/i18n";
import { hasPerm } from "@/lib/permissions";
import { buildImport, IMPORT_COLUMNS, IMPORT_TEMPLATE, type ImportRow } from "./productImport";

type Outcome = {
  created: number;
  updated: number;
  failed: { line: number; message: string }[];
};

function downloadTemplate() {
  const url = URL.createObjectURL(new Blob([IMPORT_TEMPLATE], { type: "text/csv" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = "products-template.csv";
  a.click();
  URL.revokeObjectURL(url);
}

/** Import products from a CSV file: pick, review the checked rows, then create or update them one by one. */
export function ProductImportPanel() {
  const { t, i18n } = useTranslation("products");
  const { session } = useSession();
  const canViewCost = hasPerm(session, "VIEW_COST");
  const save = useSaveProduct();
  const adjust = useAdjustStock();
  const categories = useCategories();
  const saveCategory = useSaveCategory();
  const products = useProducts();
  const input = useRef<HTMLInputElement>(null);
  const [rows, setRows] = useState<ImportRow[] | null>(null);
  const [fileError, setFileError] = useState(false);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);

  const valid = (rows ?? []).filter((r) => r.input !== null);
  const invalid = (rows ?? []).filter((r) => r.input === null);

  const pick = async (file: File | undefined) => {
    setOutcome(null);
    if (!file) return;
    const built = buildImport(await file.text(), canViewCost);
    setFileError(built === "noHeader");
    setRows(built === "noHeader" ? null : built);
  };

  const run = async () => {
    setBusy(true);
    const result: Outcome = { created: 0, updated: 0, failed: [] };

    // Build category cache (case-insensitive name -> category_id)
    const categoryCache = new Map<string, string>();
    for (const c of categories.data ?? []) {
      categoryCache.set(c.name.trim().toLowerCase(), c.id);
    }

    // Build index of existing products by barcode, SKU, and exact name
    const existingByBarcode = new Map<string, Product>();
    const existingBySku = new Map<string, Product>();
    const existingByName = new Map<string, Product>();

    for (const p of products.data ?? []) {
      if (p.barcode) existingByBarcode.set(p.barcode.trim(), p);
      if (p.sku) existingBySku.set(p.sku.trim().toLowerCase(), p);
      existingByName.set(p.name.trim().toLowerCase(), p);
    }

    for (const r of valid) {
      if (!r.input) continue;
      try {
        // Resolve category if row specified a category name
        let categoryId = r.input.category_id;
        if (r.categoryName && r.categoryName.trim() !== "") {
          const catKey = r.categoryName.trim().toLowerCase();
          const cached = categoryCache.get(catKey);
          if (cached) {
            categoryId = cached;
          } else {
            try {
              const made = await saveCategory.mutateAsync({
                id: null,
                input: { name: r.categoryName.trim(), parent_id: null },
              });
              categoryId = made.id;
              categoryCache.set(catKey, made.id);
            } catch {
              // Proceed without category if creation fails
            }
          }
        }

        const input = {
          ...r.input,
          category_id: categoryId,
        };

        // Match existing product by barcode, SKU, or name
        const rawBarcode = input.barcode.mode === "manual" ? input.barcode.value.trim() : null;
        const rawSku = input.sku.mode === "manual" ? input.sku.value.trim().toLowerCase() : null;
        const nameKey = input.name.trim().toLowerCase();

        const existing =
          (rawBarcode ? existingByBarcode.get(rawBarcode) : null) ??
          (rawSku ? existingBySku.get(rawSku) : null) ??
          existingByName.get(nameKey);

        if (existing) {
          // Update existing product
          await save.mutateAsync({ id: existing.id, input });
          result.updated += 1;

          // Adjust stock to match target quantity if specified
          const targetQty = r.quantity;
          const currentQty = existing.on_hand;
          const delta = targetQty - currentQty;
          if (delta !== 0) {
            await adjust
              .mutateAsync({
                input: { product_id: existing.id, qty_delta: delta, note: t("import.openingNote") },
                approvalId: null,
              })
              .catch(() => undefined);
          }
        } else {
          // Create new product
          const saved = await save.mutateAsync({ id: null, input });
          result.created += 1;

          if (saved.barcode) existingByBarcode.set(saved.barcode.trim(), saved);
          if (saved.sku) existingBySku.set(saved.sku.trim().toLowerCase(), saved);
          existingByName.set(saved.name.trim().toLowerCase(), saved);

          if (r.quantity > 0) {
            await adjust
              .mutateAsync({
                input: { product_id: saved.id, qty_delta: r.quantity, note: t("import.openingNote") },
                approvalId: null,
              })
              .catch(() => undefined);
          }
        }
      } catch (e) {
        result.failed.push({
          line: r.line,
          message: e instanceof CommandError ? translateError(i18n.t, e.error) : t("import.unknownError"),
        });
      }
    }
    setBusy(false);
    setOutcome(result);
    setRows(null);
    if (input.current) input.current.value = "";
  };

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-muted-foreground">
        {t("import.hint", { columns: IMPORT_COLUMNS.join(", ") })}
      </p>
      <div className="flex flex-wrap gap-2">
        <Button type="button" size="pos" variant="outline" onClick={downloadTemplate}>
          <Download aria-hidden />
          {t("import.template")}
        </Button>
        <Button type="button" size="pos" onClick={() => input.current?.click()} disabled={busy}>
          <FileUp aria-hidden />
          {t("import.choose")}
        </Button>
        <input
          ref={input}
          type="file"
          accept=".csv,text/csv"
          className="sr-only"
          aria-label={t("import.file")}
          onChange={(e) => void pick(e.target.files?.[0])}
        />
      </div>
      {fileError && (
        <p role="alert" className="text-sm text-destructive">
          {t("import.noHeader")}
        </p>
      )}
      {rows && (
        <div className="flex flex-col gap-3 rounded-xl border p-4">
          <p role="status">{t("import.summary", { valid: valid.length, invalid: invalid.length })}</p>
          {invalid.length > 0 && (
            <ul className="flex max-h-40 flex-col gap-1 overflow-y-auto text-sm text-destructive">
              {invalid.map((r) => (
                <li key={r.line}>
                  {t("import.rowIssue", {
                    line: r.line,
                    issue: t(`import.issues.${r.issue ?? "nameMissing"}`),
                  })}
                </li>
              ))}
            </ul>
          )}
          <Button
            type="button"
            size="pos"
            className="self-start"
            disabled={valid.length === 0 || busy}
            onClick={() => void run()}
          >
            {t("import.run", { count: valid.length })}
          </Button>
        </div>
      )}
      {outcome && (
        <div role="status" className="flex flex-col gap-1 rounded-xl border p-4">
          <p>
            {t("import.done", {
              created: outcome.created,
              updated: outcome.updated,
              failed: outcome.failed.length,
            })}
          </p>
          <ul className="text-sm text-destructive">
            {outcome.failed.map((f) => (
              <li key={f.line}>{t("import.rowIssue", { line: f.line, issue: f.message })}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
