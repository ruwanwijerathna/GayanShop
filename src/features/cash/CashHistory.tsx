// The Cash drawer's History tab: past shifts on the virtualised DataTable, filtered by period, counter and
// "only shifts with a difference". A cashier sees this counter; REPORT_SALES sees every counter. Open a row
// for its Z-report and cash movements, and print it again.
import type { ColumnDef } from "@tanstack/react-table";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { SelectField } from "@/components/common/SelectField";
import { DataTable } from "@/components/data-table/DataTable";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { HistoryFilter, SessionHistoryRow_Serialize as Row } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useBusinessToday, useCashHistory, useCashSessionReport } from "@/db/hooks";
import { translateError } from "@/i18n";
import { daysBefore } from "@/lib/dates";
import { formatMoney } from "@/lib/money";
import { PrintZButton } from "./PrintZButton";
import { ZReportView } from "./ZReportView";

const FIELD = "h-12 text-base";
const PAGE = 100;

type Range = "7" | "30" | "90" | "all" | "custom";
const RANGE_DAYS = { "7": 7, "30": 30, "90": 90 } as const;

function Detail({ row, onClose }: { row: Row; onClose: () => void }) {
  const { t, i18n } = useTranslation("cash");
  const report = useCashSessionReport(row.session.id);
  const error = report.error instanceof CommandError ? translateError(i18n.t, report.error.error) : null;
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>
            {t("history.detailTitle", { date: row.session.business_date, counter: row.device_code })}
          </DialogTitle>
          <DialogDescription>
            {t("history.columns.cashier")}: {row.session.opened_by_name}
          </DialogDescription>
        </DialogHeader>
        {error && (
          <p role="alert" className="text-destructive">
            {error}
          </p>
        )}
        {report.data && (
          <ZReportView
            report={report.data}
            actions={report.data.session.status === "closed" && <PrintZButton sessionId={row.session.id} />}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

export function CashHistory() {
  const today = useBusinessToday();
  return today.data ? <History today={today.data} /> : null;
}

function History({ today }: { today: string }) {
  const { t, i18n } = useTranslation(["cash", "common"]);
  const [range, setRange] = useState<Range>("30");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [deviceId, setDeviceId] = useState("");
  const [onlyDiff, setOnlyDiff] = useState(false);
  const [limit, setLimit] = useState(PAGE);
  const [open, setOpen] = useState<Row | null>(null);

  const filter = useMemo<HistoryFilter>(() => {
    const base = { device_id: deviceId || null, variance_only: onlyDiff };
    if (range === "all") return { ...base, from: null, to: null };
    if (range === "custom") return { ...base, from: from || null, to: to || null };
    return { ...base, from: daysBefore(today, RANGE_DAYS[range] - 1), to: today };
  }, [range, from, to, deviceId, onlyDiff, today]);
  const history = useCashHistory(filter, limit);
  const rows = history.data?.rows ?? [];
  const error = history.error instanceof CommandError ? translateError(i18n.t, history.error.error) : null;

  const columns = useMemo<ColumnDef<Row>[]>(() => {
    const time = (iso: string) =>
      new Date(iso).toLocaleTimeString(i18n.resolvedLanguage === "si" ? "si-LK" : "en-LK", {
        hour: "2-digit",
        minute: "2-digit",
      });
    return [
      {
        id: "date",
        header: t("history.columns.date"),
        accessorFn: (r) =>
          `${r.session.business_date} · ${time(r.session.opened_at)}–${r.session.closed_at ? time(r.session.closed_at) : t("history.status.open")}`,
        size: 220,
        enableSorting: false,
      },
      {
        id: "counter",
        header: t("history.columns.counter"),
        accessorFn: (r) => r.device_code,
        size: 90,
        enableSorting: false,
      },
      {
        id: "cashier",
        header: t("history.columns.cashier"),
        accessorFn: (r) => r.session.opened_by_name,
        size: 140,
        enableSorting: false,
      },
      {
        id: "sales",
        header: t("history.columns.sales"),
        accessorFn: (r) => `${r.sales_count} · ${formatMoney(r.sales_total)}`,
        size: 170,
        enableSorting: false,
      },
      {
        id: "counted",
        header: t("history.columns.counted"),
        accessorFn: (r) => (r.session.counted_cash === null ? "–" : formatMoney(r.session.counted_cash)),
        size: 130,
        enableSorting: false,
      },
      {
        id: "difference",
        header: t("history.columns.difference"),
        accessorFn: (r) => {
          const v = r.session.variance;
          if (v === null || v === undefined) return t("history.diff.none");
          if (v === 0) return t("history.diff.exact");
          return v < 0
            ? t("history.diff.short", { amount: formatMoney(-v) })
            : t("history.diff.over", { amount: formatMoney(v) });
        },
        size: 160,
        enableSorting: false,
      },
    ];
  }, [t, i18n]);

  const counters = history.data?.counters ?? [];
  return (
    <section className="flex flex-col gap-4" aria-label={t("history.title")}>
      <div className="grid gap-3 sm:grid-cols-3">
        <div className="flex flex-col gap-2">
          <Label htmlFor="history-range">{t("history.range.label")}</Label>
          <SelectField
            id="history-range"
            value={range}
            onValueChange={(v) => {
              setRange(v as Range);
              setLimit(PAGE);
            }}
            options={(["7", "30", "90", "all", "custom"] as const).map((r) => ({
              value: r,
              label: t(`history.range.${r}`),
            }))}
          />
        </div>
        {history.data?.all_counters && (
          <div className="flex flex-col gap-2">
            <Label htmlFor="history-counter">{t("history.counter")}</Label>
            <SelectField
              id="history-counter"
              value={deviceId}
              onValueChange={(v) => {
                setDeviceId(v);
                setLimit(PAGE);
              }}
              options={[
                { value: "", label: t("history.allCounters") },
                ...counters.map((c) => ({ value: c.device_id, label: c.device_code })),
              ]}
            />
          </div>
        )}
        <label className="flex min-h-12 items-center gap-3 self-end">
          <Checkbox
            className="size-6"
            checked={onlyDiff}
            onCheckedChange={(v) => {
              setOnlyDiff(v === true);
              setLimit(PAGE);
            }}
          />
          {t("history.onlyDifferences")}
        </label>
      </div>
      {range === "custom" && (
        <div className="grid gap-3 sm:grid-cols-3">
          <div className="flex flex-col gap-2">
            <Label htmlFor="history-from">{t("history.from")}</Label>
            <Input
              id="history-from"
              type="date"
              className={FIELD}
              value={from}
              max={to || today}
              onChange={(e) => setFrom(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="history-to">{t("history.to")}</Label>
            <Input
              id="history-to"
              type="date"
              className={FIELD}
              value={to}
              min={from || undefined}
              max={today}
              onChange={(e) => setTo(e.target.value)}
            />
          </div>
        </div>
      )}
      {error && (
        <p role="alert" className="text-base text-destructive">
          {error}
        </p>
      )}
      <DataTable<Row>
        ariaLabel={t("history.title")}
        columns={columns}
        data={rows}
        getRowId={(r) => r.session.id}
        height={480}
        rowHeight={48}
        emptyLabel={history.isLoading ? t("common:table.loading") : t("history.empty")}
        onRowActivate={setOpen}
      />
      {history.data?.truncated && (
        <button
          type="button"
          className="h-12 self-start rounded-md border border-input px-4 text-base font-medium hover:bg-accent"
          onClick={() => setLimit((l) => l + PAGE)}
        >
          {t("history.more")}
        </button>
      )}
      {open && <Detail row={open} onClose={() => setOpen(null)} />}
    </section>
  );
}
