import {
  flexRender,
  getCoreRowModel,
  getFacetedRowModel,
  getFacetedUniqueValues,
  getFilteredRowModel,
  getPaginationRowModel,
  getSortedRowModel,
  useReactTable,
  type ColumnDef,
  type ColumnFiltersState,
  type PaginationState,
  type RowSelectionState,
  type SortingState,
  type VisibilityState,
} from "@tanstack/react-table";
import {
  Barcode,
  ClipboardCheck,
  FolderTree,
  History,
  MoreHorizontal,
  Package,
  PackagePlus,
  Pencil,
  Plus,
  Search,
  Tag,
  X,
} from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "@/app/session";
import { Can } from "@/components/auth/Can";
import { DataTableColumnHeader } from "@/components/data-table/DataTableColumnHeader";
import { DataTableFacetedFilter, facetFilterFn } from "@/components/data-table/DataTableFacetedFilter";
import { DataTablePagination } from "@/components/data-table/DataTablePagination";
import { DataTableViewOptions } from "@/components/data-table/DataTableViewOptions";
import { PrintLabelsDialog } from "@/components/labels/PrintLabelsDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import type { Product } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useCategories, useProducts } from "@/db/hooks";
import { translateError } from "@/i18n";
import { formatMoney } from "@/lib/money";
import { hasPerm } from "@/lib/permissions";
import { cn } from "@/lib/utils";
import { CategoriesDialog } from "./CategoriesDialog";
import { MovementHistorySheet } from "./MovementHistorySheet";
import { categoryPath } from "./categories";
import { ProductFormDialog } from "./ProductFormDialog";
import { ShortBarcodesDialog, withoutBarcode } from "./ShortBarcodesDialog";
import { AdjustStockDialog, RestockDialog } from "./ProductStockDialogs";
import { matchesText } from "./productForm";
import { LOW_STOCK_MAX, stockLevel, type StockLevel } from "./stock";

const STOCK_LEVELS: StockLevel[] = ["in_stock", "low", "out", "oversold", "untracked"];
const NO_CATEGORY = "none";
/** Inactive products stay out of the list until the Status filter is changed or reset. */
const DEFAULT_FILTERS: ColumnFiltersState = [{ id: "status", value: ["active"] }];
const SKELETON_ROWS = 6;

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

/** Keeps a click inside a cell (checkbox, actions menu) from also reaching the row. */
const stop = (e: { stopPropagation: () => void }) => e.stopPropagation();

/** Back-office product list with live stock (bounded: loaded once, sorted / filtered / paged client-side). */
export function ProductsScreen() {
  // TanStack Table returns functions React Compiler cannot memoise safely; opt this component out.
  "use no memo";
  const { t, i18n } = useTranslation(["products", "common"]);
  const { session } = useSession();
  const canManage = hasPerm(session, "PRODUCT_MANAGE");
  const canViewCost = hasPerm(session, "VIEW_COST");
  const canSeeHistory = hasPerm(session, "REPORT_FINANCIAL");
  const canStock = hasPerm(session, "SALE_CREATE");
  const products = useProducts();
  const categories = useCategories();

  const [editing, setEditing] = useState<{ product: Product | null } | null>(null);
  const [categoriesOpen, setCategoriesOpen] = useState(false);
  const [printIds, setPrintIds] = useState<string[] | null>(null);
  const [shortBarcodesOpen, setShortBarcodesOpen] = useState(false);
  const [historyFor, setHistoryFor] = useState<Product | null>(null);
  const [restockFor, setRestockFor] = useState<Product | null>(null);
  const [adjustFor, setAdjustFor] = useState<Product | null>(null);

  const [sorting, setSorting] = useState<SortingState>([{ id: "name", desc: false }]);
  const [columnFilters, setColumnFilters] = useState<ColumnFiltersState>(DEFAULT_FILTERS);
  const [globalFilter, setGlobalFilter] = useState("");
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>({});
  const [rowSelection, setRowSelection] = useState<RowSelectionState>({});
  const [pagination, setPagination] = useState<PaginationState>({ pageIndex: 0, pageSize: 25 });

  const cats = useMemo(() => categories.data ?? [], [categories.data]);
  const uncoded = useMemo(() => withoutBarcode(products.data ?? []), [products.data]);
  const catName = useMemo(() => {
    const m = new Map<string, string>();
    for (const c of cats) m.set(c.id, categoryPath(c, cats));
    return m;
  }, [cats]);
  const data = useMemo(() => products.data ?? [], [products.data]);

  const columns = useMemo<ColumnDef<Product>[]>(() => {
    const cols: ColumnDef<Product>[] = [
      {
        id: "select",
        enableSorting: false,
        enableHiding: false,
        header: ({ table }) => (
          <Checkbox
            className="size-6"
            aria-label={t("selectPage")}
            checked={
              table.getIsAllPageRowsSelected() || (table.getIsSomePageRowsSelected() && "indeterminate")
            }
            onCheckedChange={(v) => table.toggleAllPageRowsSelected(v === true)}
          />
        ),
        cell: ({ row }) => (
          <Checkbox
            className="size-6"
            aria-label={t("selectProduct", { name: row.original.name })}
            checked={row.getIsSelected()}
            onCheckedChange={(v) => row.toggleSelected(v === true)}
          />
        ),
      },
      {
        id: "name",
        accessorFn: (p) => p.name,
        enableHiding: false,
        header: ({ column }) => <DataTableColumnHeader column={column} title={t("columns.name")} />,
        cell: ({ row }) => {
          const p = row.original;
          const codes = [p.sku, p.barcode].filter((v): v is string => Boolean(v));
          return (
            <div className="flex min-w-0 flex-col gap-0.5 py-1">
              {canManage ? (
                <button
                  type="button"
                  className="w-full truncate rounded-sm text-left font-medium outline-none hover:underline focus-visible:ring-[3px] focus-visible:ring-ring/50"
                  onClick={() => setEditing({ product: p })}
                >
                  {p.name}
                </button>
              ) : (
                <span className="truncate font-medium">{p.name}</span>
              )}
              {codes.length > 0 && (
                <span className="truncate font-mono text-xs text-muted-foreground">{codes.join(" · ")}</span>
              )}
            </div>
          );
        },
      },
      {
        id: "category",
        accessorFn: (p) => p.category_id ?? NO_CATEGORY,
        filterFn: facetFilterFn,
        sortingFn: (a, b) =>
          (catName.get(a.original.category_id ?? "") ?? "").localeCompare(
            catName.get(b.original.category_id ?? "") ?? "",
          ),
        header: ({ column }) => <DataTableColumnHeader column={column} title={t("columns.category")} />,
        cell: ({ row }) => {
          const id = row.original.category_id;
          return id ? (
            <span className="truncate">{catName.get(id) ?? ""}</span>
          ) : (
            <span className="text-muted-foreground">{t("uncategorized")}</span>
          );
        },
      },
      {
        id: "price",
        accessorFn: (p) => p.price,
        header: ({ column }) => (
          <DataTableColumnHeader column={column} title={t("columns.price")} className="justify-end" />
        ),
        cell: ({ row }) => (
          <span className="block text-right font-mono text-sm tabular-nums">
            {formatMoney(row.original.price)}
          </span>
        ),
      },
    ];
    if (canViewCost) {
      cols.push({
        id: "cost",
        accessorFn: (p) => p.default_cost ?? 0,
        header: ({ column }) => (
          <DataTableColumnHeader column={column} title={t("columns.cost")} className="justify-end" />
        ),
        cell: ({ row }) => (
          <span className="block text-right font-mono text-sm tabular-nums text-muted-foreground">
            {formatMoney(row.original.default_cost ?? 0)}
          </span>
        ),
      });
    }
    cols.push(
      {
        id: "stock",
        // Filtering works on the level; sorting on the count (untracked first, then by units on hand).
        accessorFn: (p) => stockLevel(p),
        filterFn: facetFilterFn,
        sortingFn: (a, b) => {
          const n = (p: Product) => (p.track_stock ? p.on_hand : Number.MIN_SAFE_INTEGER);
          return n(a.original) - n(b.original);
        },
        header: ({ column }) => <DataTableColumnHeader column={column} title={t("columns.stock")} />,
        cell: ({ row }) => <StockCell product={row.original} />,
      },
      {
        id: "status",
        accessorFn: (p) => (p.is_active ? "active" : "inactive"),
        filterFn: facetFilterFn,
        enableSorting: false,
        header: () => t("columns.status"),
        cell: ({ row }) =>
          row.original.barcode_conflict ? (
            <Badge variant="destructive">{t("status.conflict")}</Badge>
          ) : row.original.is_active ? (
            <Badge variant="secondary" className="gap-1.5">
              <span aria-hidden className="size-1.5 rounded-full bg-emerald-500" />
              {t("status.active")}
            </Badge>
          ) : (
            <Badge variant="outline" className="gap-1.5 text-muted-foreground">
              <span aria-hidden className="size-1.5 rounded-full bg-muted-foreground/50" />
              {t("status.inactive")}
            </Badge>
          ),
      },
      {
        id: "actions",
        enableSorting: false,
        enableHiding: false,
        header: () => <span className="sr-only">{t("columns.actions")}</span>,
        cell: ({ row }) => (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="icon-pos"
                className="rounded-xl data-[state=open]:bg-accent"
                aria-label={t("rowActions", { name: row.original.name })}
              >
                <MoreHorizontal aria-hidden />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {canStock && row.original.track_stock && (
                <>
                  <DropdownMenuLabel>{t("stock.menuLabel")}</DropdownMenuLabel>
                  <DropdownMenuItem className="min-h-11" onSelect={() => setRestockFor(row.original)}>
                    <PackagePlus aria-hidden />
                    {t("stock.restock.menu")}
                  </DropdownMenuItem>
                  <DropdownMenuItem className="min-h-11" onSelect={() => setAdjustFor(row.original)}>
                    <ClipboardCheck aria-hidden />
                    {t("stock.adjust.menu")}
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                </>
              )}
              {canManage && (
                <DropdownMenuItem className="min-h-11" onSelect={() => setEditing({ product: row.original })}>
                  <Pencil aria-hidden />
                  {t("edit")}
                </DropdownMenuItem>
              )}
              {canSeeHistory && (
                <DropdownMenuItem className="min-h-11" onSelect={() => setHistoryFor(row.original)}>
                  <History aria-hidden />
                  {t("movementHistory")}
                </DropdownMenuItem>
              )}
              <DropdownMenuItem className="min-h-11" onSelect={() => setPrintIds([row.original.id])}>
                <Tag aria-hidden />
                {t("printLabel")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        ),
      },
    );
    return cols;
  }, [t, canViewCost, canManage, canSeeHistory, canStock, catName]);

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Table v8 is not React-Compiler-safe; opted out above
  const table = useReactTable({
    data,
    columns,
    getRowId: (p) => p.id,
    state: { sorting, columnFilters, globalFilter, columnVisibility, rowSelection, pagination },
    onSortingChange: setSorting,
    onColumnFiltersChange: setColumnFilters,
    onGlobalFilterChange: setGlobalFilter,
    onColumnVisibilityChange: setColumnVisibility,
    onRowSelectionChange: setRowSelection,
    onPaginationChange: setPagination,
    globalFilterFn: (row, _columnId, value) => matchesText(row.original, String(value ?? "")),
    getColumnCanGlobalFilter: (column) => column.id === "name",
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getFacetedRowModel: getFacetedRowModel(),
    getFacetedUniqueValues: getFacetedUniqueValues(),
    getPaginationRowModel: getPaginationRowModel(),
  });

  const filteredCount = table.getFilteredRowModel().rows.length;
  const selectedIds = Object.keys(rowSelection).filter((id) => rowSelection[id]);
  const isFiltered = columnFilters.length > 0 || globalFilter !== "";
  const visibleColumns = table.getVisibleLeafColumns().length;

  const loadError =
    (products.error ?? categories.error) instanceof CommandError
      ? translateError(i18n.t, ((products.error ?? categories.error) as CommandError).error)
      : null;

  const columnLabels = {
    category: t("columns.category"),
    price: t("columns.price"),
    cost: t("columns.cost"),
    stock: t("columns.stock"),
    status: t("columns.status"),
  };

  return (
    <section className="flex flex-col gap-4">
      <header className="flex flex-wrap items-center gap-3">
        <div className="flex min-w-0 flex-1 items-center gap-3">
          <span className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary">
            <Package aria-hidden className="size-5" />
          </span>
          <div className="min-w-0">
            <h2 className="text-lg font-semibold leading-tight">{t("title")}</h2>
            <p role="status" className="text-sm text-muted-foreground tabular-nums">
              {t("count", { count: filteredCount })}
            </p>
          </div>
        </div>
        <Button
          type="button"
          size="pos"
          variant="outline"
          className="rounded-xl"
          onClick={() => setCategoriesOpen(true)}
        >
          <FolderTree aria-hidden />
          {t("categories.title")}
          <Badge variant="secondary" className="ml-1 rounded-md px-1.5 tabular-nums">
            {cats.length}
          </Badge>
        </Button>
        <Can perm="PRODUCT_MANAGE">
          {uncoded.length > 0 && (
            <Button
              type="button"
              size="pos"
              variant="outline"
              className="rounded-xl"
              onClick={() => setShortBarcodesOpen(true)}
            >
              <Barcode aria-hidden />
              {t("shortBarcodes.open", { count: uncoded.length })}
            </Button>
          )}
          <Button
            type="button"
            size="pos"
            className="rounded-xl shadow-sm"
            onClick={() => setEditing({ product: null })}
          >
            <Plus aria-hidden />
            {t("add")}
          </Button>
        </Can>
      </header>

      {loadError && (
        <p role="alert" className="rounded-xl bg-destructive px-4 py-3 text-destructive-foreground">
          {loadError}
        </p>
      )}

      <Card className="gap-0 overflow-hidden py-0 shadow-sm">
        <div className="flex flex-wrap items-center gap-2 border-b bg-muted/20 p-3">
          <div className="relative min-w-56 flex-1">
            <Search
              aria-hidden
              className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-muted-foreground"
            />
            <Input
              type="search"
              aria-label={t("searchLabel")}
              placeholder={t("search")}
              className="h-12 rounded-xl bg-background pl-10 text-base"
              value={globalFilter}
              autoComplete="off"
              onChange={(e) => setGlobalFilter(e.target.value)}
            />
          </div>
          <DataTableFacetedFilter
            column={table.getColumn("category")}
            title={t("filters.category")}
            options={[
              ...cats.map((c) => ({ value: c.id, label: catName.get(c.id) ?? c.name })),
              { value: NO_CATEGORY, label: t("uncategorized") },
            ]}
          />
          <DataTableFacetedFilter
            column={table.getColumn("stock")}
            title={t("filters.stock")}
            options={STOCK_LEVELS.map((l) => ({
              value: l,
              label: t(`stockLevel.${l}`, { max: LOW_STOCK_MAX }),
            }))}
          />
          <DataTableFacetedFilter
            column={table.getColumn("status")}
            title={t("filters.status")}
            options={[
              { value: "active", label: t("status.active") },
              { value: "inactive", label: t("status.inactive") },
            ]}
          />
          {isFiltered && (
            <Button
              type="button"
              variant="ghost"
              size="pos"
              className="rounded-xl"
              onClick={() => {
                setColumnFilters([]);
                setGlobalFilter("");
              }}
            >
              {t("filters.reset")}
              <X aria-hidden />
            </Button>
          )}
          <div className="ml-auto">
            <DataTableViewOptions table={table} labels={columnLabels} />
          </div>
        </div>

        {selectedIds.length > 0 && (
          <div className="flex flex-wrap items-center gap-3 border-b bg-primary/5 px-4 py-2 motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-top-1">
            <span className="flex-1 text-sm font-medium tabular-nums">
              {t("selectedCount", { count: selectedIds.length })}
            </span>
            <Button
              type="button"
              size="pos"
              variant="outline"
              className="rounded-xl bg-background"
              onClick={() => setPrintIds(selectedIds)}
            >
              <Tag aria-hidden />
              {t("printLabels", { count: selectedIds.length })}
            </Button>
            <Button
              type="button"
              size="pos"
              variant="ghost"
              className="rounded-xl"
              onClick={() => setRowSelection({})}
            >
              {t("clearSelection")}
            </Button>
          </div>
        )}

        <Table role="grid" aria-label={t("title")} className="min-w-[760px]">
          <TableHeader className="bg-muted/40">
            {table.getHeaderGroups().map((group) => (
              <TableRow key={group.id} className="hover:bg-transparent">
                {group.headers.map((header) => (
                  <TableHead
                    key={header.id}
                    scope="col"
                    aria-sort={
                      header.column.getIsSorted() === "asc"
                        ? "ascending"
                        : header.column.getIsSorted() === "desc"
                          ? "descending"
                          : undefined
                    }
                    className={cn(
                      "h-12",
                      header.column.id === "select" && "w-12 pl-4",
                      (header.column.id === "price" || header.column.id === "cost") && "text-right",
                      header.column.id === "actions" && "w-14 text-right pr-4",
                    )}
                  >
                    {header.isPlaceholder
                      ? null
                      : flexRender(header.column.columnDef.header, header.getContext())}
                  </TableHead>
                ))}
              </TableRow>
            ))}
          </TableHeader>
          <TableBody>
            {products.isLoading &&
              Array.from({ length: SKELETON_ROWS }, (_, i) => (
                <TableRow key={i} className="h-16 hover:bg-transparent" aria-hidden>
                  {Array.from({ length: visibleColumns }, (_, j) => (
                    <TableCell key={j}>
                      <Skeleton className="h-5 w-full max-w-32" />
                    </TableCell>
                  ))}
                </TableRow>
              ))}
            {!products.isLoading &&
              table.getRowModel().rows.map((row, i) => (
                <TableRow
                  key={row.id}
                  data-state={row.getIsSelected() ? "selected" : undefined}
                  className={cn(
                    "h-16 motion-safe:animate-in motion-safe:fade-in-0",
                    !row.original.is_active && "text-muted-foreground",
                  )}
                  style={{ animationDelay: `${Math.min(i, 10) * 20}ms`, animationFillMode: "backwards" }}
                >
                  {row.getVisibleCells().map((cell) => {
                    const id = cell.column.id;
                    return (
                      <TableCell
                        key={cell.id}
                        className={cn(
                          id === "select" && "w-12 pl-4",
                          id === "name" && "max-w-64",
                          (id === "price" || id === "cost") && "text-right",
                          id === "actions" && "w-14 text-right pr-4",
                        )}
                        onClick={id === "select" || id === "actions" ? stop : undefined}
                      >
                        {flexRender(cell.column.columnDef.cell, cell.getContext())}
                      </TableCell>
                    );
                  })}
                </TableRow>
              ))}
            {!products.isLoading && table.getRowModel().rows.length === 0 && (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={visibleColumns} className="py-16">
                  <div className="flex flex-col items-center gap-2 text-center text-muted-foreground">
                    <span className="flex size-12 items-center justify-center rounded-full bg-muted">
                      <Package aria-hidden className="size-6" />
                    </span>
                    <span>{data.length === 0 ? t("empty") : t("noMatch")}</span>
                  </div>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
        {products.isLoading && <p className="sr-only">{t("common:table.loading")}</p>}
        <DataTablePagination table={table} />
      </Card>

      <CategoriesDialog open={categoriesOpen} onOpenChange={setCategoriesOpen} categories={cats} />
      {shortBarcodesOpen && (
        <ShortBarcodesDialog
          products={uncoded}
          onClose={() => setShortBarcodesOpen(false)}
          onDone={(ids) => setRowSelection(Object.fromEntries(ids.map((id) => [id, true])))}
        />
      )}
      <ProductFormDialog
        open={editing !== null}
        product={editing?.product ?? null}
        categories={cats}
        onClose={() => setEditing(null)}
      />
      {restockFor && <RestockDialog product={restockFor} onClose={() => setRestockFor(null)} />}
      {adjustFor && <AdjustStockDialog product={adjustFor} onClose={() => setAdjustFor(null)} />}
      {historyFor && <MovementHistorySheet product={historyFor} onClose={() => setHistoryFor(null)} />}
      {printIds && (
        <PrintLabelsDialog
          items={data
            .filter((p) => printIds.includes(p.id))
            .map((p) => ({ product_id: p.id, name: p.name, copies: 1 }))}
          onClose={() => {
            setPrintIds(null);
            setRowSelection({});
          }}
        />
      )}
    </section>
  );
}
