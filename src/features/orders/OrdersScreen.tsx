// Bills: every sealed bill, newest first, on the virtualised DataTable with server-side keyset paging.
// One big search box finds a bill by receipt number or a customer's phone (across all dates); the day
// chips and one Filters dialog narrow the rest. Tap a bill to see its receipt and print it again.
// A cashier sees this counter's bills; REPORT_SALES sees every counter (Rust enforces it).
import type { ColumnDef } from "@tanstack/react-table";
import { FileDown, Search, SlidersHorizontal } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Can } from "@/components/auth/Can";
import { SelectField } from "@/components/common/SelectField";
import { DataTable } from "@/components/data-table/DataTable";
import { ExportButton } from "@/components/export/ExportButton";
import { ReceiptDialog } from "@/components/receipt/ReceiptDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
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

type Quick = "today" | "yesterday" | "week" | "month" | "date";
const QUICK: Quick[] = ["today", "yesterday", "week", "month", "date"];
const METHODS = ["cash", "card", "bank_transfer", "qr", "other"] as const;
type Method = (typeof METHODS)[number];
const DATE = /^\d{4}-\d{2}-\d{2}$/;
const SEARCH_DELAY_MS = 250;
const BIG = "h-14 text-lg";

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

/** A list height that fills the window under the controls. */
function useListHeight(): number {
  const [h, setH] = useState(() => Math.max(320, window.innerHeight - 330));
  useEffect(() => {
    const onResize = () => setH(Math.max(320, window.innerHeight - 330));
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return h;
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
  onChange: (e: Extras) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("orders");
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("filters.title")}</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-4">
          <Can perm="REPORT_SALES">
            <CounterFilter
              value={extras.deviceId}
              onChange={(deviceId) => onChange({ ...extras, deviceId })}
            />
          </Can>
          <div className="flex flex-col gap-2">
            <Label htmlFor="bills-payment">{t("filters.payment")}</Label>
            <SelectField
              id="bills-payment"
              value={extras.payment}
              onValueChange={(v) => onChange({ ...extras, payment: isMethod(v) ? v : "" })}
              options={[
                { value: "", label: t("filters.anyPayment") },
                ...METHODS.map((m) => ({ value: m, label: t(`method.${m}`) })),
              ]}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="bills-kind">{t("filters.kind")}</Label>
            <SelectField
              id="bills-kind"
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
          <Button type="button" size="pos" variant="outline" onClick={() => onChange(NO_EXTRAS)}>
            {t("filters.clear")}
          </Button>
          <Button type="button" size="pos" onClick={onClose}>
            {t("filters.done")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Bills({ today }: { today: string }) {
  const { t, i18n } = useTranslation(["orders", "common"]);
  const [text, setText] = useState("");
  const [quick, setQuick] = useState<Quick>("today");
  const [picked, setPicked] = useState(today);
  const [extras, setExtras] = useState<Extras>(NO_EXTRAS);
  const [filtering, setFiltering] = useState(false);
  const [open, setOpen] = useState<string | null>(null);
  const listHeight = useListHeight();

  const terms = searchTerms(useDebounced(text, SEARCH_DELAY_MS));
  const searching = terms.receipt !== null || terms.phone !== null;
  const dayPeriod = periodOf(quick, today, picked);
  // Looking for one bill should never depend on remembering the day it was sold.
  const period: Period = searching ? { kind: "all_time" } : dayPeriod;
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
  const singleDay = period.kind === "day";
  const locale = i18n.resolvedLanguage === "si" ? "si-LK" : "en-LK";

  const columns = useMemo<ColumnDef<OrderRow>[]>(
    () => [
      {
        id: "bill",
        header: t("columns.bill"),
        accessorFn: (r) => r.receipt_no,
        enableSorting: false,
        size: 340,
        cell: ({ row }) => {
          const at = new Date(row.original.completed_at);
          const when = singleDay
            ? at.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })
            : at.toLocaleString(locale, { dateStyle: "medium", timeStyle: "short" });
          return (
            <span className="flex flex-col leading-tight">
              <span className="text-base font-medium">{when}</span>
              <span className="font-mono text-xs text-muted-foreground">{row.original.receipt_no}</span>
            </span>
          );
        },
      },
      {
        id: "paid",
        header: t("columns.paid"),
        accessorFn: (r) => r.methods,
        enableSorting: false,
        size: 200,
        cell: ({ row }) =>
          row.original.methods
            .split(",")
            .filter(isMethod)
            .map((m) => t(`method.${m}`))
            .join(" + "),
      },
      {
        id: "tags",
        header: "",
        accessorFn: (r) => r.status,
        enableSorting: false,
        size: 260,
        cell: ({ row }) => (
          <span className="flex flex-wrap gap-1.5">
            {row.original.status === "voided" && <Badge variant="destructive">{t("badge.voided")}</Badge>}
            {row.original.exchange_credit > 0 && <Badge variant="secondary">{t("badge.exchange")}</Badge>}
            {row.original.has_returns && <Badge variant="outline">{t("badge.returns")}</Badge>}
          </span>
        ),
      },
      {
        id: "total",
        header: t("columns.total"),
        accessorFn: (r) => r.grand_total,
        enableSorting: false,
        size: 160,
        cell: ({ row }) => (
          <span
            className={
              row.original.status === "voided"
                ? "w-full text-right font-semibold tabular-nums text-muted-foreground line-through"
                : "w-full text-right font-semibold tabular-nums"
            }
          >
            {formatMoney(row.original.grand_total)}
          </span>
        ),
      },
    ],
    [t, singleDay, locale],
  );

  const error = pages.error instanceof CommandError ? translateError(i18n.t, pages.error.error) : null;
  return (
    <section className="flex flex-col gap-4">
      <header className="flex flex-wrap items-center gap-3">
        <h1 className="flex-1 text-2xl font-semibold">{t("title")}</h1>
        <Can perm="DATA_EXPORT">
          <Popover>
            <PopoverTrigger asChild>
              <Button type="button" size="pos" variant="outline">
                <FileDown aria-hidden />
                {t("export")}
              </Button>
            </PopoverTrigger>
            <PopoverContent align="end" className="flex w-auto flex-col gap-2">
              <ExportButton kind="orders" period={dayPeriod} />
              <ExportButton kind="order_lines" period={dayPeriod} />
            </PopoverContent>
          </Popover>
        </Can>
      </header>

      <div className="relative">
        <Search
          aria-hidden
          className="pointer-events-none absolute top-1/2 left-4 size-5 -translate-y-1/2 text-muted-foreground"
        />
        <Input
          type="search"
          aria-label={t("search.label")}
          placeholder={t("search.placeholder")}
          className={`${BIG} pl-12`}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      </div>
      {terms.tooShort && <p className="-mt-2 text-sm text-muted-foreground">{t("search.tooShort")}</p>}

      <div className="flex flex-wrap items-center gap-2">
        {searching ? (
          <p className="flex-1 text-base text-muted-foreground">{t("search.allDates")}</p>
        ) : (
          <div
            role="group"
            aria-label={t("period.label")}
            className="flex flex-1 flex-wrap items-center gap-2"
          >
            {QUICK.map((q) => (
              <Button
                key={q}
                type="button"
                size="pos"
                variant={quick === q ? "default" : "outline"}
                aria-pressed={quick === q}
                onClick={() => setQuick(q)}
              >
                {t(`period.${q}`)}
              </Button>
            ))}
            {quick === "date" && (
              <Input
                type="date"
                aria-label={t("period.date")}
                className="h-12 w-auto text-base"
                value={picked}
                max={today}
                onChange={(e) => DATE.test(e.target.value) && setPicked(e.target.value)}
              />
            )}
          </div>
        )}
        <Button type="button" size="pos" variant="outline" onClick={() => setFiltering(true)}>
          <SlidersHorizontal aria-hidden />
          {activeFilters > 0 ? t("filters.buttonCount", { count: activeFilters }) : t("filters.button")}
        </Button>
        {activeFilters > 0 && (
          <Button type="button" size="pos" variant="ghost" onClick={() => setExtras(NO_EXTRAS)}>
            {t("filters.clear")}
          </Button>
        )}
      </div>

      {error && (
        <p role="alert" className="text-base text-destructive">
          {error}
        </p>
      )}
      <DataTable<OrderRow>
        ariaLabel={t("title")}
        columns={columns}
        data={rows}
        getRowId={(r) => r.id}
        height={listHeight}
        rowHeight={48}
        emptyLabel={
          pages.isLoading
            ? t("common:table.loading")
            : searching || activeFilters > 0
              ? t("emptyFiltered")
              : t("empty")
        }
        onRowActivate={(r) => setOpen(r.id)}
        onEndReached={() => {
          if (pages.hasNextPage && !pages.isFetchingNextPage) void pages.fetchNextPage();
        }}
      />
      {filtering && (
        <FiltersDialog extras={extras} onChange={setExtras} onClose={() => setFiltering(false)} />
      )}
      {open && <Receipt orderId={open} onClose={() => setOpen(null)} />}
    </section>
  );
}

export function OrdersScreen() {
  const today = useBusinessToday();
  return today.data ? <Bills today={today.data} /> : null;
}
