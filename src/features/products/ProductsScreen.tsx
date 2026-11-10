import type { ColumnDef } from "@tanstack/react-table";
import { Plus, Search, Tag } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "@/app/session";
import { Can } from "@/components/auth/Can";
import { DataTable } from "@/components/data-table/DataTable";
import { ExportButton } from "@/components/export/ExportButton";
import { PrintLabelsDialog } from "@/components/labels/PrintLabelsDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { Product } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useCategories, useProducts } from "@/db/hooks";
import { translateError } from "@/i18n";
import { formatMoney } from "@/lib/money";
import { hasPerm } from "@/lib/permissions";
import { CategoryPanel, categoryPath } from "./CategoryPanel";
import { ProductFormDialog } from "./ProductFormDialog";
import { matchesText } from "./productForm";
import { LOW_STOCK_MAX, matchesStockFilter, stockLevel, type StockFilter } from "./stock";

/**
 * Column widths (px). With the cost column they must fit the 1024×768 grid (≈ 895 px usable, see the audit
 * table); a 13-digit barcode in the mono font needs ~135 px.
 */
export const PRODUCT_COLUMN_WIDTHS = {
  select: 48,
  name: 150,
  sku: 84,
  barcode: 135,
  category: 90,
  price: 96,
  cost: 96,
  stock: 112,
  status: 82,
} as const;

const STOCK_FILTERS: StockFilter[] = ["all", "low", "out"];

/** The Stock cell: plain count, or a badge for low / out / oversold; a dash when stock isn't tracked. */
function StockCell({ product }: { product: Product }) {
  const { t } = useTranslation("products");
  const level = stockLevel(product);
  const n = product.on_hand;
  if (level === "untracked")
    return (
      <span className="text-muted-foreground" title={t("stockCell.untracked")}>
        <span aria-hidden>—</span>
        <span className="sr-only">{t("stockCell.untracked")}</span>
      </span>
    );
  if (level === "in_stock")
    return <span className="text-sm tabular-nums">{t("stockCell.inStock", { count: n })}</span>;
  if (level === "low")
    return (
      <Badge className="bg-warning text-warning-foreground tabular-nums">
        {t("stockCell.low", { count: n })}
      </Badge>
    );
  if (level === "out")
    return (
      <Badge variant="outline" className="border-destructive text-destructive tabular-nums">
        {t("stockCell.out")}
      </Badge>
    );
  return (
    <Badge variant="destructive" className="tabular-nums">
      {t("stockCell.oversold", { count: n })}
    </Badge>
  );
}

type Editing = { product: Product | null } | null;

/** Back-office product list with live stock (bounded: loaded once, filtered client-side) plus categories. */
export function ProductsScreen() {
  const { t, i18n } = useTranslation(["products", "common"]);
  const { session } = useSession();
  const canManage = hasPerm(session, "PRODUCT_MANAGE");
  const canViewCost = hasPerm(session, "VIEW_COST");
  const products = useProducts();
  const categories = useCategories();
  const [text, setText] = useState("");
  const [categoryId, setCategoryId] = useState("");
  const [showInactive, setShowInactive] = useState(false);
  const [stockFilter, setStockFilter] = useState<StockFilter>("all");
  const [editing, setEditing] = useState<Editing>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [printing, setPrinting] = useState(false);

  const cats = useMemo(() => categories.data ?? [], [categories.data]);
  const catName = useMemo(() => {
    const m = new Map<string, string>();
    for (const c of cats) m.set(c.id, categoryPath(c, cats));
    return m;
  }, [cats]);

  const all = products.data;
  const rows = useMemo(
    () =>
      (all ?? []).filter(
        (p) =>
          (showInactive || p.is_active) &&
          (categoryId === "" || p.category_id === categoryId) &&
          matchesStockFilter(p, stockFilter) &&
          matchesText(p, text),
      ),
    [all, showInactive, categoryId, stockFilter, text],
  );

  const columns = useMemo<ColumnDef<Product>[]>(() => {
    const cols: ColumnDef<Product>[] = [
      {
        id: "select",
        header: () => <span className="sr-only">{t("selectForLabels")}</span>,
        size: PRODUCT_COLUMN_WIDTHS.select,
        enableSorting: false,
        accessorFn: (p) => p.id,
        cell: ({ row }) => (
          <input
            type="checkbox"
            className="size-6 accent-primary"
            aria-label={t("selectProduct", { name: row.original.name })}
            checked={selected.has(row.original.id)}
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => e.stopPropagation()}
            onChange={(e) => {
              const id = row.original.id;
              const on = e.target.checked;
              setSelected((prev) => {
                const next = new Set(prev);
                if (on) next.add(id);
                else next.delete(id);
                return next;
              });
            }}
          />
        ),
      },
      {
        id: "name",
        header: t("columns.name"),
        size: PRODUCT_COLUMN_WIDTHS.name,
        accessorFn: (p) => p.name,
        cell: ({ row }) => <span className="truncate">{row.original.name}</span>,
      },
      {
        id: "sku",
        header: t("columns.sku"),
        size: PRODUCT_COLUMN_WIDTHS.sku,
        accessorFn: (p) => p.sku ?? "",
        cell: ({ getValue }) => <span className="font-mono text-sm">{String(getValue())}</span>,
      },
      {
        id: "barcode",
        header: t("columns.barcode"),
        size: PRODUCT_COLUMN_WIDTHS.barcode,
        accessorFn: (p) => p.barcode ?? "",
        cell: ({ getValue }) => <span className="font-mono text-sm">{String(getValue())}</span>,
      },
      {
        id: "category",
        header: t("columns.category"),
        size: PRODUCT_COLUMN_WIDTHS.category,
        accessorFn: (p) => (p.category_id ? (catName.get(p.category_id) ?? "") : ""),
      },
      {
        id: "price",
        header: t("columns.price"),
        size: PRODUCT_COLUMN_WIDTHS.price,
        accessorFn: (p) => p.price,
        cell: ({ row }) => <span className="font-mono text-sm">{formatMoney(row.original.price)}</span>,
      },
    ];
    if (canViewCost) {
      cols.push({
        id: "cost",
        header: t("columns.cost"),
        size: PRODUCT_COLUMN_WIDTHS.cost,
        accessorFn: (p) => p.default_cost ?? 0,
        cell: ({ row }) => (
          <span className="font-mono text-sm">{formatMoney(row.original.default_cost ?? 0)}</span>
        ),
      });
    }
    cols.push({
      id: "stock",
      header: t("columns.stock"),
      size: PRODUCT_COLUMN_WIDTHS.stock,
      // Untracked sorts first, then by units on hand.
      accessorFn: (p) => (p.track_stock ? p.on_hand : Number.MIN_SAFE_INTEGER),
      cell: ({ row }) => <StockCell product={row.original} />,
    });
    cols.push({
      id: "status",
      header: t("columns.status"),
      size: PRODUCT_COLUMN_WIDTHS.status,
      enableSorting: false,
      accessorFn: (p) => (p.is_active ? 1 : 0),
      cell: ({ row }) => (
        <span className="flex flex-wrap gap-1">
          {row.original.barcode_conflict ? (
            <Badge variant="destructive">{t("status.conflict")}</Badge>
          ) : row.original.is_active ? (
            <Badge variant="secondary">{t("status.active")}</Badge>
          ) : (
            <Badge variant="outline">{t("status.inactive")}</Badge>
          )}
        </span>
      ),
    });
    return cols;
  }, [t, canViewCost, catName, selected]);

  const loadError =
    (products.error ?? categories.error) instanceof CommandError
      ? translateError(i18n.t, ((products.error ?? categories.error) as CommandError).error)
      : null;

  const emptyLabel = products.isLoading
    ? t("common:table.loading")
    : (all?.length ?? 0) === 0
      ? t("empty")
      : t("noMatch");

  return (
    <section className="flex flex-col gap-4">
      <header className="flex flex-wrap items-center gap-3">
        <h2 className="text-xl font-semibold">{t("title")}</h2>
        <span role="status" className="flex-1 text-sm text-muted-foreground">
          {t("count", { count: rows.length })}
        </span>
        <ExportButton kind="products" period={{ kind: "all_time" }} />
        <Button
          type="button"
          size="pos"
          variant="outline"
          disabled={selected.size === 0}
          onClick={() => setPrinting(true)}
        >
          <Tag aria-hidden />
          {t("printLabels", { count: selected.size })}
        </Button>
        <Can perm="PRODUCT_MANAGE">
          <Button type="button" size="pos" onClick={() => setEditing({ product: null })}>
            <Plus aria-hidden />
            {t("add")}
          </Button>
        </Can>
      </header>
      <div className="flex flex-wrap items-center gap-3">
        <div className="relative min-w-64 flex-1">
          <Search
            aria-hidden
            className="pointer-events-none absolute top-3.5 left-3 size-5 text-muted-foreground"
          />
          <Input
            type="search"
            aria-label={t("searchLabel")}
            placeholder={t("search")}
            className="h-12 pl-10 text-base"
            value={text}
            autoComplete="off"
            onChange={(e) => setText(e.target.value)}
          />
        </div>
        <select
          aria-label={t("categoryFilter")}
          className="h-12 rounded-md border border-input bg-background px-3 text-base"
          value={categoryId}
          onChange={(e) => setCategoryId(e.target.value)}
        >
          <option value="">{t("allCategories")}</option>
          {cats.map((c) => (
            <option key={c.id} value={c.id}>
              {catName.get(c.id)}
            </option>
          ))}
        </select>
        <div role="group" aria-label={t("stockFilter.label")} className="flex flex-wrap gap-2">
          {STOCK_FILTERS.map((f) => (
            <Button
              key={f}
              type="button"
              size="pos"
              variant={stockFilter === f ? "default" : "outline"}
              aria-pressed={stockFilter === f}
              onClick={() => setStockFilter(f)}
            >
              {t(`stockFilter.${f}`, { max: LOW_STOCK_MAX })}
            </Button>
          ))}
        </div>
        <label className="flex min-h-12 items-center gap-2">
          <input
            type="checkbox"
            className="size-6 accent-primary"
            checked={showInactive}
            onChange={(e) => setShowInactive(e.target.checked)}
          />
          {t("showInactive")}
        </label>
      </div>
      {loadError && (
        <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
          {loadError}
        </p>
      )}
      <div className="grid grid-cols-1 gap-4 2xl:grid-cols-[1fr_22rem]">
        <DataTable<Product>
          ariaLabel={t("title")}
          columns={columns}
          data={rows}
          getRowId={(p) => p.id}
          height={480}
          emptyLabel={emptyLabel}
          onRowActivate={canManage ? (p) => setEditing({ product: p }) : undefined}
        />
        <CategoryPanel categories={cats} />
      </div>
      <ProductFormDialog
        open={editing !== null}
        product={editing?.product ?? null}
        categories={cats}
        onClose={() => setEditing(null)}
      />
      {printing && (
        <PrintLabelsDialog
          items={(all ?? [])
            .filter((p) => selected.has(p.id))
            .map((p) => ({ product_id: p.id, name: p.name, copies: 1 }))}
          onClose={() => {
            setPrinting(false);
            setSelected(new Set());
          }}
        />
      )}
    </section>
  );
}
