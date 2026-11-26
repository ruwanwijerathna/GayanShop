// Bills: every sealed bill, newest first, in the same card-and-table layout as Products & Stock, with
// server-side keyset paging ("Load more"). All bills show by default; the period buttons narrow them.
// One search box finds a bill by receipt number or a customer's phone (across all dates); counter,
// payment, type and status filter inline. Tap or press Enter on a bill to see its receipt and print it again.
// A cashier sees this counter's bills; REPORT_SALES sees every counter (Rust enforces it).
import {
  flexRender,
  getCoreRowModel,
  getPaginationRowModel,
  useReactTable,
  type ColumnDef,
  type PaginationState,
  type VisibilityState,
} from "@tanstack/react-table";
import { FileDown, ReceiptText, Search, SlidersHorizontal, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Can } from "@/components/auth/Can";
import { SelectField } from "@/components/common/SelectField";
import { DataTableColumnHeader } from "@/components/data-table/DataTableColumnHeader";
import { DataTablePagination } from "@/components/data-table/DataTablePagination";
import { DataTableViewOptions } from "@/components/data-table/DataTableViewOptions";
import { ExportButton } from "@/components/export/ExportButton";
import { ReceiptDialog } from "@/components/receipt/ReceiptDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import type {
  BillKind,
  OrderFilter_Deserialize as OrderFilter,
  OrderRow_Serialize as OrderRow,
  Period,
} from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useBusinessToday, useOrderHistory, useOrderReceipt, useReportDevices } from "@/db/hooks";
import { translateError } from "@/i18n";
import { daysBefore } from "@/lib/dates";
import { formatMoney } from "@/lib/money";
import { cn } from "@/lib/utils";

type Quick = "all" | "today" | "yesterday" | "week" | "month" | "date";
const QUICK: Quick[] = ["all", "today", "yesterday", "week", "month", "date"];
const METHODS = ["cash", "card", "bank_transfer", "qr", "other"] as const;
type Method = (typeof METHODS)[number];
const DATE = /^\d{4}-\d{2}-\d{2}$/;
const SEARCH_DELAY_MS = 250;
const SKELETON_ROWS = 8;
const FILTER_FIELD = "w-auto min-w-40 rounded-xl";
/** Columns the View menu can hide (the receipt number and the row action always stay). */
const HIDEABLE = ["date", "cashier", "items", "payment", "status", "total"] as const;

interface Extras {
  deviceId: string;
  payment: Method | "";
  kind: BillKind | "";
  voidedOnly: boolean;
}
const NO_EXTRAS: Extras = { deviceId: "", payment: "", kind: "", voidedOnly: false };

function isMethod(v: string): v is Method {
  return METHODS.some((m) => m === v);
}

/** Receipt numbers start with a letter (`T01-…`); anything else typed is part of a phone number. */
export function searchTerms(text: string): {
  receipt: string | null;
  phone: string | null;
  tooShort: boolean;
} {
  const q = text.trim();
  if (q === "") return { receipt: null, phone: null, tooShort: false };
  if (/^[a-z]/i.test(q)) return { receipt: q, phone: null, tooShort: false };
  const digits = q.replace(/\D/g, "");
  return digits.length >= 4
    ? { receipt: null, phone: q, tooShort: false }
    : { receipt: null, phone: null, tooShort: true };
}

function periodOf(quick: Quick, today: string, picked: string): Period {
  switch (quick) {
    case "all":
      return { kind: "all_time" };
    case "today":
      return { kind: "day", date: today };
    case "yesterday":
      return { kind: "day", date: daysBefore(today, 1) };
    case "week":
      return { kind: "week", any_date: today };
    case "month":
      return {
        kind: "month",
        year: Number.parseInt(today.slice(0, 4), 10),
        month: Number.parseInt(today.slice(5, 7), 10),
      };
    case "date":
      return { kind: "day", date: picked };
  }
}

function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const id = window.setTimeout(() => setV(value), ms);
    return () => window.clearTimeout(id);
  }, [value, ms]);
  return v;
}

function Receipt({ orderId, onClose }: { orderId: string; onClose: () => void }) {
  const receipt = useOrderReceipt(orderId);
  return <ReceiptDialog receipt={receipt.data} error={receipt.error} onClose={onClose} />;
}

function CounterFilter({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const { t } = useTranslation("orders");
  const devices = useReportDevices();
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor="bills-counter">{t("filters.counter")}</Label>
      <SelectField
        id="bills-counter"
        className="w-full rounded-xl"
        value={value}
        onValueChange={onChange}
        options={[
          { value: "", label: t("filters.allCounters") },
          ...(devices.data ?? []).map((d) => ({ value: d.device_id, label: d.device_code })),
        ]}
      />
    </div>
  );
}

function FiltersDialog({
  extras,
  onChange,
  onClose,
}: {
  extras: Extras;
  onChange: (next: Extras) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("orders");
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("filters.title")}</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-4 py-2">
          <Can perm="REPORT_SALES">
            <CounterFilter value={extras.deviceId} onChange={(deviceId) => onChange({ ...extras, deviceId })} />
          </Can>
          <div className="flex flex-col gap-2">
            <Label htmlFor="bills-kind">{t("filters.kind")}</Label>
            <SelectField
              id="bills-kind"
              className="w-full rounded-xl"
              value={extras.kind}
              onValueChange={(v) =>
                onChange({ ...extras, kind: v === "exchange" || v === "with_returns" ? v : "" })
              }
              options={[
                { value: "", label: t("filters.anyKind") },
                { value: "exchange", label: t("filters.exchange") },
                { value: "with_returns", label: t("filters.withReturns") },
              ]}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="bills-status">{t("filters.status")}</Label>
            <SelectField
              id="bills-status"
              className="w-full rounded-xl"
              value={extras.voidedOnly ? "voided" : ""}
              onValueChange={(v) => onChange({ ...extras, voidedOnly: v === "voided" })}
              options={[
                { value: "", label: t("filters.anyStatus") },
                { value: "voided", label: t("filters.voidedOnly") },
              ]}
            />
          </div>
        </div>
        <DialogFooter className="flex-row justify-between gap-2 sm:justify-between">
          <Button
            type="button"
            size="pos"
            variant="outline"
            className="rounded-xl"
            onClick={() => onChange({ ...extras, deviceId: "", kind: "", voidedOnly: false })}
          >
            {t("filters.clear")}
          </Button>
          <Button type="button" size="pos" className="rounded-xl" onClick={onClose}>
            {t("filters.done")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** A bill number scanned elsewhere in the app; `at` tells two scans of the same bill apart. */
export interface ScannedBill {
  receipt: string;
  at: number;
}

function Bills({ today, scanned }: { today: string; scanned: ScannedBill | null }) {
  "use no memo";
  const { t, i18n } = useTranslation(["orders", "common", "reports"]);
  const [text, setText] = useState("");
  const [quick, setQuick] = useState<Quick>("all");
  const [picked, setPicked] = useState(today);
  const [extras, setExtras] = useState<Extras>(NO_EXTRAS);
  const [filtering, setFiltering] = useState(false);
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>({});
  const [pagination, setPagination] = useState<PaginationState>({ pageIndex: 0, pageSize: 25 });
  const [open, setOpen] = useState<string | null>(null);
  const [wanted, setWanted] = useState<string | null>(null);

  // A scanned bill fills the search box (all dates) and opens as soon as it is in the list.
  const [seen, setSeen] = useState<ScannedBill | null>(null);
  if (scanned && (scanned.at !== seen?.at || scanned.receipt !== seen.receipt)) {
    setSeen(scanned);
    setText(scanned.receipt);
    setWanted(scanned.receipt.toUpperCase());
  }

  const terms = searchTerms(useDebounced(text, SEARCH_DELAY_MS));
  const searching = terms.receipt !== null || terms.phone !== null;
  const chosenPeriod = periodOf(quick, today, picked);
  // Looking for one bill should never depend on remembering the day it was sold.
  const period: Period = searching ? { kind: "all_time" } : chosenPeriod;
  const filter: OrderFilter = {
    period,
    device_id: extras.deviceId || null,
    status: extras.voidedOnly ? "voided" : null,
    receipt: terms.receipt,
    phone: terms.phone,
    payment: extras.payment || null,
    kind: extras.kind || null,
  };
  const pages = useOrderHistory(filter);
  const rows = useMemo(() => pages.data?.pages.flatMap((p) => p.rows) ?? [], [pages.data]);
  const activeFilters = [extras.deviceId, extras.payment, extras.kind, extras.voidedOnly ? "v" : ""].filter(
    Boolean,
  ).length;
  const dialogFilters = [extras.deviceId, extras.kind, extras.voidedOnly ? "v" : ""].filter(Boolean).length;
  const resetPage = () => setPagination((p) => (p.pageIndex === 0 ? p : { ...p, pageIndex: 0 }));
  const settled =
    wanted !== null && terms.receipt?.toUpperCase() === wanted && !!pages.data && !pages.isFetching;
  const hit = settled ? rows.find((r) => r.receipt_no.toUpperCase() === wanted) : undefined;
  const missed = useRef<ScannedBill | null>(null);
  useEffect(() => {
    if (!settled || hit || !seen || missed.current === seen) return;
    missed.current = seen;
    toast.error(t("search.scannedNotFound", { receipt: seen.receipt }), { duration: 4_000 });
  }, [settled, hit, seen, t]);
  const locale = i18n.resolvedLanguage === "si" ? "si-LK" : "en-LK";

  const columnLabels: Record<string, string> = Object.fromEntries(
    HIDEABLE.map((id) => [id, t(`columns.${id}`)]),
  );

  const columns = useMemo<ColumnDef<OrderRow>[]>(
    () => [
      {
        id: "receipt",
        accessorFn: (r) => r.receipt_no,
        enableHiding: false,
        header: ({ column }) => <DataTableColumnHeader column={column} title={t("columns.receipt")} />,
        cell: ({ row }) => {
          const r = row.original;
          return (
            <span className="flex flex-col gap-0.5 leading-tight">
              <span
                className={cn(
                  "font-mono text-sm font-semibold",
                  r.status === "voided" && "text-muted-foreground",
                )}
              >
                {r.receipt_no}
              </span>
              {r.status === "voided" && r.void_reason && (
                <span className="text-xs text-destructive">
                  {t(`reports:voids.reasons.${r.void_reason}`, { defaultValue: r.void_reason })}
                </span>
              )}
            </span>
          );
        },
      },
      {
        id: "date",
        accessorFn: (r) => r.completed_at,
        header: ({ column }) => <DataTableColumnHeader column={column} title={t("columns.date")} />,
        cell: ({ row }) => {
          const at = new Date(row.original.completed_at);
          return (
            <span className="flex flex-col leading-tight tabular-nums">
              <span className="text-sm font-medium whitespace-nowrap">
                {at.toLocaleDateString(locale, { dateStyle: "medium" })}
              </span>
              <span className="text-xs whitespace-nowrap text-muted-foreground">
                {at.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })}
              </span>
            </span>
          );
        },
      },
      {
        id: "cashier",
        accessorFn: (r) => r.cashier ?? "",
        header: ({ column }) => <DataTableColumnHeader column={column} title={t("columns.cashier")} />,
        cell: ({ row }) => <span className="text-sm">{row.original.cashier ?? "—"}</span>,
      },
      {
        id: "items",
        accessorFn: (r) => r.item_count,
        header: ({ column }) => (
          <DataTableColumnHeader column={column} title={t("columns.items")} className="justify-end" />
        ),
        cell: ({ row }) => (
          <span className="block text-right text-sm tabular-nums">{row.original.item_count}</span>
        ),
      },
      {
        id: "payment",
        accessorFn: (r) => r.methods,
        header: ({ column }) => <DataTableColumnHeader column={column} title={t("columns.payment")} />,
        cell: ({ row }) => {
          const r = row.original;
          const methods = r.methods.split(",").filter(isMethod);
          return (
            <span className="flex flex-col items-start gap-1">
              <span className="flex flex-wrap gap-1">
                {methods.map((m) => (
                  <Badge key={m} variant="outline" className="font-normal">
                    {t(`method.${m}`)}
                  </Badge>
                ))}
              </span>
              {r.exchange_credit > 0 && (
                <span className="text-xs whitespace-nowrap text-muted-foreground tabular-nums">
                  {t("exchangeCredit", { amount: formatMoney(r.exchange_credit) })}
                </span>
              )}
            </span>
          );
        },
      },
      {
        id: "status",
        accessorFn: (r) => r.status,
        header: ({ column }) => <DataTableColumnHeader column={column} title={t("columns.status")} />,
        cell: ({ row }) => {
          const r = row.original;
          return (
            <span className="flex flex-wrap gap-1.5">
              {r.status === "voided" ? (
                <Badge variant="destructive">{t("badge.voided")}</Badge>
              ) : (
                <Badge variant="secondary" className="font-normal">
                  {t("badge.completed")}
                </Badge>
              )}
              {r.exchange_credit > 0 && <Badge variant="secondary">{t("badge.exchange")}</Badge>}
              {r.has_returns && <Badge variant="outline">{t("badge.returns")}</Badge>}
            </span>
          );
        },
      },
      {
        id: "total",
        accessorFn: (r) => r.grand_total,
        header: ({ column }) => (
          <DataTableColumnHeader column={column} title={t("columns.total")} className="justify-end" />
        ),
        cell: ({ row }) => (
          <span
            className={cn(
              "block text-right font-mono font-semibold whitespace-nowrap tabular-nums",
              row.original.status === "voided" && "text-muted-foreground line-through",
            )}
          >
            {formatMoney(row.original.grand_total)}
          </span>
        ),
      },
      {
        id: "actions",
        enableHiding: false,
        header: () => <span className="sr-only">{t("columns.actions")}</span>,
        cell: ({ row }) => (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-11 rounded-xl text-muted-foreground"
            aria-label={t("actions.view", { receipt: row.original.receipt_no })}
            onClick={(e) => {
              e.stopPropagation();
              setOpen(row.original.id);
            }}
          >
            <ReceiptText aria-hidden />
          </Button>
        ),
      },
    ],
    [t, locale],
  );

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Table v8 is not React-Compiler-safe; opted out above
  const table = useReactTable({
    data: rows,
    columns,
    getRowId: (r) => r.id,
    state: { columnVisibility, pagination },
    onColumnVisibilityChange: setColumnVisibility,
    onPaginationChange: setPagination,
    getCoreRowModel: getCoreRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    autoResetPageIndex: false,
    enableSorting: false,
  });
  const visibleColumns = table.getVisibleLeafColumns().length;
  const loading = pages.isLoading;
  const pageCount = table.getPageCount();
  useEffect(() => {
    if (pages.hasNextPage && !pages.isFetchingNextPage && pageCount > 0 && pagination.pageIndex >= pageCount - 1) {
      void pages.fetchNextPage();
    }
  }, [pages, pageCount, pagination.pageIndex]);

  const activate = (e: KeyboardEvent<HTMLTableRowElement>, id: string) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      setOpen(id);
    }
  };

  const error = pages.error instanceof CommandError ? translateError(i18n.t, pages.error.error) : null;
  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight sm:text-3xl">{t("title")}</h1>
          <p className="text-sm text-muted-foreground">{t("subtitle")}</p>
        </div>
        <Can perm="DATA_EXPORT">
          <Popover>
            <PopoverTrigger asChild>
              <Button type="button" size="pos" variant="outline" className="shrink-0 rounded-xl">
                <FileDown aria-hidden />
                {t("export")}
              </Button>
            </PopoverTrigger>
            <PopoverContent align="end" className="flex w-auto flex-col gap-2">
              <ExportButton kind="orders" period={chosenPeriod} />
              <ExportButton kind="order_lines" period={chosenPeriod} />
            </PopoverContent>
          </Popover>
        </Can>
      </header>

      {error && (
        <p role="alert" className="text-base text-destructive">
          {error}
        </p>
      )}

      <Card className="min-w-0 gap-0 overflow-hidden py-0 shadow-sm">
        <div className="flex flex-col gap-3 border-b bg-muted/20 p-3">
          <div className="flex flex-wrap items-center gap-2">
            <div className="relative min-w-56 flex-1">
              <Search
                aria-hidden
                className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-muted-foreground"
              />
              <Input
                type="search"
                aria-label={t("search.label")}
                placeholder={t("search.placeholder")}
                className="h-12 rounded-xl bg-background pl-10 text-base"
                value={text}
                autoComplete="off"
                onChange={(e) => {
                  setText(e.target.value);
                  resetPage();
                }}
              />
            </div>
            <SelectField
              aria-label={t("filters.payment")}
              className={FILTER_FIELD}
              value={extras.payment}
              onValueChange={(v) => {
                setExtras({ ...extras, payment: isMethod(v) ? v : "" });
                resetPage();
              }}
              options={[
                { value: "", label: t("filters.anyPayment") },
                ...METHODS.map((m) => ({ value: m, label: t(`method.${m}`) })),
              ]}
            />
            {!searching && (
              <>
                <SelectField
                  aria-label={t("period.label")}
                  className={FILTER_FIELD}
                  value={quick}
                  onValueChange={(v) => {
                    const next = QUICK.find((q) => q === v);
                    if (next) {
                      setQuick(next);
                      resetPage();
                    }
                  }}
                  options={QUICK.map((q) => ({ value: q, label: t(`period.${q}`) }))}
                />
                {quick === "date" && (
                  <Input
                    type="date"
                    aria-label={t("period.date")}
                    className="h-12 w-auto rounded-xl bg-background text-base"
                    value={picked}
                    max={today}
                    onChange={(e) => {
                      if (DATE.test(e.target.value)) {
                        setPicked(e.target.value);
                        resetPage();
                      }
                    }}
                  />
                )}
              </>
            )}
            <Button
              type="button"
              variant="outline"
              size="pos"
              className="rounded-xl bg-background"
              onClick={() => setFiltering(true)}
            >
              <SlidersHorizontal aria-hidden />
              {dialogFilters > 0 ? t("filters.buttonCount", { count: dialogFilters }) : t("filters.button")}
            </Button>
            {(activeFilters > 0 || quick !== "all") && (
              <Button
                type="button"
                variant="ghost"
                size="pos"
                className="rounded-xl"
                onClick={() => {
                  setExtras(NO_EXTRAS);
                  setQuick("all");
                  resetPage();
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

          {terms.tooShort && <p className="text-sm text-muted-foreground">{t("search.tooShort")}</p>}
          {searching && <p className="text-sm text-muted-foreground">{t("search.allDates")}</p>}
        </div>

        <Table role="grid" aria-label={t("title")} className="min-w-[860px]">
          <TableHeader className="bg-muted/40">
            {table.getHeaderGroups().map((group) => (
              <TableRow key={group.id} className="hover:bg-transparent">
                {group.headers.map((header) => (
                  <TableHead
                    key={header.id}
                    scope="col"
                    className={cn(
                      "h-12",
                      header.column.id === "receipt" && "pl-4",
                      (header.column.id === "items" || header.column.id === "total") && "text-right",
                      header.column.id === "actions" && "w-14 pr-4 text-right",
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
            {loading &&
              Array.from({ length: SKELETON_ROWS }, (_, i) => (
                <TableRow key={i} className="h-14 hover:bg-transparent" aria-hidden>
                  {Array.from({ length: visibleColumns }, (_, j) => (
                    <TableCell key={j} className="py-2">
                      <Skeleton className="h-5 w-full max-w-32" />
                    </TableCell>
                  ))}
                </TableRow>
              ))}
            {!loading &&
              table.getRowModel().rows.map((row, i) => (
                <TableRow
                  key={row.id}
                  tabIndex={0}
                  className={cn(
                    "h-14 cursor-pointer outline-none focus-visible:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset motion-safe:animate-in motion-safe:fade-in-0",
                    row.original.status === "voided" && "bg-destructive/5",
                  )}
                  style={{ animationDelay: `${Math.min(i, 10) * 20}ms`, animationFillMode: "backwards" }}
                  onClick={() => setOpen(row.original.id)}
                  onKeyDown={(e) => activate(e, row.original.id)}
                >
                  {row.getVisibleCells().map((cell) => {
                    const id = cell.column.id;
                    return (
                      <TableCell
                        key={cell.id}
                        className={cn(
                          "py-2",
                          id === "receipt" && "pl-4",
                          (id === "items" || id === "total") && "text-right",
                          id === "actions" && "w-14 pr-4 text-right",
                        )}
                      >
                        {flexRender(cell.column.columnDef.cell, cell.getContext())}
                      </TableCell>
                    );
                  })}
                </TableRow>
              ))}
            {!loading && rows.length === 0 && (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={visibleColumns} className="py-16">
                  <div className="flex flex-col items-center gap-2 text-center text-muted-foreground">
                    <span className="flex size-12 items-center justify-center rounded-full bg-muted">
                      <ReceiptText aria-hidden className="size-6" />
                    </span>
                    <span>
                      {searching || activeFilters > 0
                        ? t("emptyFiltered")
                        : quick === "all"
                          ? t("emptyAll")
                          : t("empty")}
                    </span>
                  </div>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
        {loading && <p className="sr-only">{t("common:table.loading")}</p>}

        <DataTablePagination table={table} />
      </Card>

      {filtering && (
        <FiltersDialog
          extras={extras}
          onChange={(next) => {
            setExtras(next);
            resetPage();
          }}
          onClose={() => setFiltering(false)}
        />
      )}

      {(open ?? hit?.id) && (
        <Receipt
          orderId={open ?? hit?.id ?? ""}
          onClose={() => {
            setOpen(null);
            setWanted(null);
          }}
        />
      )}
    </section>
  );
}

export function OrdersScreen({ scanned = null }: { scanned?: ScannedBill | null }) {
  const today = useBusinessToday();
  return today.data ? <Bills today={today.data} scanned={scanned} /> : null;
}
