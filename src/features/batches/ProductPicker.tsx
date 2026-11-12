import { Search } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { ProductSummary } from "@/db/bindings";
import { useProductSearch } from "@/db/hooks";
import { formatMoney } from "@/lib/money";

/** Type-ahead product chooser over the Rust FTS search (English, SKU, barcode or Sinhala). */
export function ProductPicker({
  value,
  onChange,
}: {
  value: ProductSummary | null;
  onChange: (product: ProductSummary | null) => void;
}) {
  const { t } = useTranslation("batches");
  const [query, setQuery] = useState("");
  const search = useProductSearch(query);
  const listId = useId();

  if (value) {
    return (
      <div className="flex min-h-12 items-center gap-3 rounded-md border border-border px-3">
        <span className="flex-1">
          {t("picker.selected", { name: value.name })}
          {value.sku && <span className="ml-2 font-mono text-sm text-muted-foreground">{value.sku}</span>}
        </span>
        <Button type="button" size="pos" variant="outline" onClick={() => onChange(null)}>
          {t("picker.change")}
        </Button>
      </div>
    );
  }

  const results = search.data ?? [];
  return (
    <div className="flex flex-col gap-2">
      <div className="relative">
        <Search
          aria-hidden
          className="pointer-events-none absolute top-3.5 left-3 size-5 text-muted-foreground"
        />
        <Input
          type="search"
          aria-label={t("picker.label")}
          aria-controls={listId}
          placeholder={t("picker.search")}
          className="h-12 pl-10 text-base"
          value={query}
          autoComplete="off"
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      {query.trim() !== "" && (
        <ul id={listId} className="flex max-h-60 flex-col overflow-y-auto rounded-md border border-border">
          {results.length === 0 && !search.isFetching ? (
            <li className="px-3 py-3 text-muted-foreground">{t("picker.noResults")}</li>
          ) : (
            results.map((p) => (
              <li key={p.id}>
                <button
                  type="button"
                  className="flex min-h-12 w-full items-center gap-3 px-3 text-left hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
                  onClick={() => {
                    onChange(p);
                    setQuery("");
                  }}
                >
                  <span className="flex-1">{p.name}</span>
                  <span className="font-mono text-sm text-muted-foreground">{p.sku ?? p.barcode}</span>
                  <span className="font-mono text-sm">{formatMoney(p.price)}</span>
                </button>
              </li>
            ))
          )}
        </ul>
      )}
    </div>
  );
}
