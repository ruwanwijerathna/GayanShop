import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Kbd } from "@/components/common/Kbd";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import type { ProductSummary_Serialize as ProductSummary } from "@/db/bindings";
import { useCategories, useProductDetails } from "@/db/hooks";
import { formatMoney } from "@/lib/money";
import { cn } from "@/lib/utils";

/**
 * Several products share one manufacturer barcode: pick the right one. Keyboard first: ↑ ↓ move, Enter adds
 * the highlighted product, Esc adds nothing; a click or touch on a row works too. Focus lands on the list
 * the moment the dialog opens, so the cashier never needs the mouse.
 */
export function BarcodeDisambiguationDialog({
  code,
  products,
  onSelect,
  onClose,
}: {
  code: string;
  products: readonly ProductSummary[];
  onSelect: (product: ProductSummary) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("pos");
  const listRef = useRef<HTMLUListElement>(null);
  const [index, setIndex] = useState(0);
  const categories = useCategories().data ?? [];
  const details = useProductDetails(products.map((p) => p.id));

  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>('[aria-selected="true"]')
      ?.scrollIntoView?.({ block: "nearest" });
  }, [index]);

  const onKeyDown = (e: KeyboardEvent<HTMLUListElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setIndex((i) => Math.min(i + 1, products.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      const pick = products[index];
      if (pick) onSelect(pick);
    }
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        className="max-w-xl"
        onOpenAutoFocus={(e) => {
          e.preventDefault();
          listRef.current?.focus();
        }}
      >
        <DialogHeader>
          <DialogTitle>{t("disambiguation.title")}</DialogTitle>
          <DialogDescription>
            {t("disambiguation.description", { code, count: products.length })}
          </DialogDescription>
        </DialogHeader>
        <ul
          ref={listRef}
          role="listbox"
          tabIndex={0}
          aria-label={t("disambiguation.title")}
          aria-activedescendant={`variant-${index}`}
          onKeyDown={onKeyDown}
          className="flex max-h-96 flex-col gap-2 overflow-y-auto rounded-md p-1 outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {products.map((p, i) => {
            const detail = details[i]?.data;
            const category = detail?.category_id
              ? categories.find((c) => c.id === detail.category_id)
              : undefined;
            const active = i === index;
            return (
              <li
                key={p.id}
                id={`variant-${i}`}
                role="option"
                aria-selected={active}
                onMouseEnter={() => setIndex(i)}
                onMouseDown={(e) => {
                  e.preventDefault();
                  onSelect(p);
                }}
                className={cn(
                  "flex min-h-14 cursor-pointer items-center gap-3 rounded-md border px-3 py-2",
                  active ? "border-primary bg-primary/10 text-primary" : "hover:bg-muted/50",
                )}
              >
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-base font-medium">{p.name}</span>
                  <span className="truncate text-sm text-muted-foreground">
                    {[
                      category?.name ?? null,
                      p.sku,
                      detail && p.track_stock ? t("disambiguation.stock", { count: detail.on_hand }) : null,
                    ]
                      .filter((x): x is string => Boolean(x))
                      .join(" · ")}
                  </span>
                </div>
                <span className="font-mono text-lg font-semibold">{formatMoney(p.price)}</span>
              </li>
            );
          })}
        </ul>
        <p className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
          <Kbd>↑</Kbd>
          <Kbd>↓</Kbd>
          {t("disambiguation.move")}
          <Kbd>Enter</Kbd>
          {t("disambiguation.add")}
          <Kbd>Esc</Kbd>
          {t("disambiguation.cancel")}
        </p>
      </DialogContent>
    </Dialog>
  );
}
