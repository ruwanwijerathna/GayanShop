// Reports (doc §5, REPORT_SALES): any §5.2 period, a counter filter and the previous equivalent period for
// comparison. The §5.1 metrics (P&L lines need REPORT_FINANCIAL + VIEW_COST; Rust leaves them empty
// otherwise), payments by method, sales by product / category / batch, and voids by reason and approver.
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { PeriodPicker, periodFor } from "@/components/period/PeriodPicker";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import type { FinancialSummary_Serialize as FinancialSummary, Period, SalesDimension } from "@/db/bindings";
import { CommandError } from "@/db/client";
import {
  useBusinessToday,
  useReportDevices,
  useReportSalesBy,
  useReportSummary,
  useReportVoids,
} from "@/db/hooks";
import { translateError } from "@/i18n";
import { formatMoney } from "@/lib/money";
import { cn } from "@/lib/utils";

const SELECT = "h-12 w-full rounded-md border border-input bg-background px-3 text-base";
const DIMENSIONS: SalesDimension[] = ["product", "category", "batch"];

/** `2_550` bp → `25.5%` (negative margins keep their sign). */
export function formatBp(bp: number): string {
  const sign = bp < 0 ? "-" : "";
  const abs = Math.abs(bp);
  return `${sign}${Math.trunc(abs / 100)}.${Math.trunc((abs % 100) / 10)}%`;
}

/** Change vs the previous period in basis points (`null` when there is nothing to compare with). */
export function changeBp(current: number, previous: number): number | null {
  if (previous === 0) return null;
  return Math.trunc(((current - previous) * 10_000) / Math.abs(previous));
}

type Line = { key: string; money: boolean; get: (s: FinancialSummary) => number | null; strong?: boolean };

const LINES: Line[] = [
  { key: "sales_count", money: false, get: (s) => s.sales_count },
  { key: "gross_sales", money: true, get: (s) => s.gross_sales },
  { key: "discounts", money: true, get: (s) => s.discounts },
  { key: "net_sales", money: true, get: (s) => s.net_sales, strong: true },
  { key: "tax_collected", money: true, get: (s) => s.tax_collected },
  { key: "avg_basket", money: true, get: (s) => s.avg_basket },
  { key: "cogs", money: true, get: (s) => s.cogs },
  { key: "gross_profit", money: true, get: (s) => s.gross_profit, strong: true },
  { key: "stock_write_offs", money: true, get: (s) => s.stock_write_offs },
  { key: "operating_expenses", money: true, get: (s) => s.operating_expenses },
  { key: "net_profit", money: true, get: (s) => s.net_profit, strong: true },
  { key: "quick_sale_net", money: true, get: (s) => s.quick_sale_net },
  { key: "unknown_cost_net", money: true, get: (s) => s.unknown_cost_net },
  { key: "void_count", money: false, get: (s) => s.void_count },
  { key: "void_value", money: true, get: (s) => s.void_value },
];

function ErrorText({ error }: { error: unknown }) {
  const { i18n } = useTranslation();
  if (!(error instanceof CommandError)) return null;
  return (
    <p role="alert" className="text-base text-destructive">
      {translateError(i18n.t, error.error)}
    </p>
  );
}

function fmt(v: number | null, money: boolean): string {
  if (v === null) return "—";
  return money ? formatMoney(v) : String(v);
}

function Pnl({ current, previous }: { current: FinancialSummary; previous: FinancialSummary | null }) {
  const { t } = useTranslation("reports");
  const lines = LINES.filter((l) => l.get(current) !== null);
  return (
    <table className="w-full text-left text-base" aria-label={t("pnl.title")}>
      <thead className="border-b text-sm">
        <tr>
          <th className="py-2">{t("pnl.metric")}</th>
          <th className="text-right">{t("pnl.current")}</th>
          {previous && <th className="text-right">{t("pnl.previous")}</th>}
          {previous && <th className="text-right">{t("pnl.change")}</th>}
        </tr>
      </thead>
      <tbody>
        {lines.map((l) => {
          const cur = l.get(current);
          const prev = previous ? l.get(previous) : null;
          const change = cur !== null && prev !== null ? changeBp(cur, prev) : null;
          return (
            <tr key={l.key} className={cn("h-11 border-b", l.strong && "font-semibold")}>
              <td>{t(`metrics.${l.key}`)}</td>
              <td className="text-right font-mono">{fmt(cur, l.money)}</td>
              {previous && (
                <td className="text-right font-mono text-muted-foreground">{fmt(prev, l.money)}</td>
              )}
              {previous && (
                <td className="text-right font-mono">{change === null ? "—" : formatBp(change)}</td>
              )}
            </tr>
          );
        })}
        {current.gross_margin_bp !== null && (
          <tr className="h-11 border-b">
            <td>{t("metrics.gross_margin")}</td>
            <td className="text-right font-mono">{formatBp(current.gross_margin_bp)}</td>
            {previous && (
              <td className="text-right font-mono text-muted-foreground">
                {previous.gross_margin_bp === null ? "—" : formatBp(previous.gross_margin_bp)}
              </td>
            )}
            {previous && <td />}
          </tr>
        )}
        <tr className="h-11 border-b">
          <td>{t("metrics.customer_attached")}</td>
          <td className="text-right font-mono">{formatBp(current.customer_attached_bp)}</td>
          {previous && (
            <td className="text-right font-mono text-muted-foreground">
              {formatBp(previous.customer_attached_bp)}
            </td>
          )}
          {previous && <td />}
        </tr>
      </tbody>
    </table>
  );
}

function SalesBy({ period, deviceId }: { period: Period; deviceId: string | null }) {
  const { t } = useTranslation("reports");
  const [dimension, setDimension] = useState<SalesDimension>("product");
  const rows = useReportSalesBy(period, deviceId, dimension);
  const none = t(`salesBy.none.${dimension}`);
  return (
    <section className="flex flex-col gap-3" aria-label={t("salesBy.title")}>
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="flex-1 text-xl font-semibold">{t("salesBy.title")}</h2>
        {DIMENSIONS.map((d) => (
          <Button
            key={d}
            type="button"
            size="pos"
            variant={dimension === d ? "default" : "outline"}
            aria-pressed={dimension === d}
            onClick={() => setDimension(d)}
          >
            {t(`salesBy.by.${d}`)}
          </Button>
        ))}
      </div>
      <ErrorText error={rows.error} />
      {rows.data && rows.data.length === 0 && <p className="text-muted-foreground">{t("empty")}</p>}
      {rows.data && rows.data.length > 0 && (
        <table className="w-full text-left text-sm">
          <thead className="border-b">
            <tr>
              <th className="py-2">{t(`salesBy.by.${dimension}`)}</th>
              <th className="text-right">{t("salesBy.qty")}</th>
              <th className="text-right">{t("salesBy.net")}</th>
              {rows.data.some((r) => r.cost !== null) && (
                <th className="text-right">{t("salesBy.profit")}</th>
              )}
            </tr>
          </thead>
          <tbody>
            {rows.data.map((r) => (
              <tr key={r.id ?? "none"} className="h-10 border-b">
                <td>{r.label ?? none}</td>
                <td className="text-right font-mono">{r.qty}</td>
                <td className="text-right font-mono">{formatMoney(r.net)}</td>
                {r.cost !== null && <td className="text-right font-mono">{formatMoney(r.net - r.cost)}</td>}
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

function Voids({ period, deviceId }: { period: Period; deviceId: string | null }) {
  const { t } = useTranslation("reports");
  const v = useReportVoids(period, deviceId);
  if (!v.data) return <ErrorText error={v.error} />;
  return (
    <section className="flex flex-col gap-3" aria-label={t("voids.title")}>
      <h2 className="text-xl font-semibold">{t("voids.title")}</h2>
      <p className="text-base">
        {t("voids.summary", { count: v.data.count, value: formatMoney(v.data.value) })}
      </p>
      {v.data.count > 0 && (
        <div className="grid gap-4 sm:grid-cols-2">
          <table className="w-full text-left text-sm" aria-label={t("voids.byReason")}>
            <thead className="border-b">
              <tr>
                <th className="py-2">{t("voids.byReason")}</th>
                <th className="text-right">{t("voids.count")}</th>
                <th className="text-right">{t("voids.value")}</th>
              </tr>
            </thead>
            <tbody>
              {v.data.by_reason.map((g) => (
                <tr key={g.key} className="h-10 border-b">
                  <td>{t(`voids.reasons.${g.key}`)}</td>
                  <td className="text-right font-mono">{g.count}</td>
                  <td className="text-right font-mono">{formatMoney(g.value)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <table className="w-full text-left text-sm" aria-label={t("voids.byApprover")}>
            <thead className="border-b">
              <tr>
                <th className="py-2">{t("voids.byApprover")}</th>
                <th className="text-right">{t("voids.count")}</th>
                <th className="text-right">{t("voids.value")}</th>
              </tr>
            </thead>
            <tbody>
              {v.data.by_approver.map((g) => (
                <tr key={g.key} className="h-10 border-b">
                  <td>{g.label ?? g.key}</td>
                  <td className="text-right font-mono">{g.count}</td>
                  <td className="text-right font-mono">{formatMoney(g.value)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function Reports({ today }: { today: string }) {
  const { t } = useTranslation("reports");
  const [period, setPeriod] = useState<Period>(() => periodFor("day", today));
  const [deviceId, setDeviceId] = useState("");
  const [compare, setCompare] = useState(true);
  const devices = useReportDevices();
  const summary = useReportSummary(period, deviceId || null, compare && period.kind !== "all_time");
  const s = summary.data;
  return (
    <section className="flex flex-col gap-6">
      <h1 className="text-2xl font-semibold">{t("title")}</h1>
      <PeriodPicker value={period} onChange={setPeriod} today={today} />
      <div className="flex flex-wrap items-end gap-4">
        <div className="flex min-w-48 flex-col gap-2">
          <Label htmlFor="report-device">{t("counter")}</Label>
          <select
            id="report-device"
            className={SELECT}
            value={deviceId}
            onChange={(e) => setDeviceId(e.target.value)}
          >
            <option value="">{t("wholeStore")}</option>
            {(devices.data ?? []).map((d) => (
              <option key={d.device_id} value={d.device_id}>
                {d.device_code}
              </option>
            ))}
          </select>
        </div>
        <label className="flex min-h-12 items-center gap-3 text-base">
          <input
            type="checkbox"
            className="size-6"
            checked={compare}
            disabled={period.kind === "all_time"}
            onChange={(e) => setCompare(e.target.checked)}
          />
          {t("compare")}
        </label>
      </div>
      {deviceId !== "" && <p className="text-sm text-muted-foreground">{t("storeWideNote")}</p>}
      <ErrorText error={summary.error} />
      {s && (
        <div className="grid gap-6 xl:grid-cols-[1fr_24rem]">
          <section className="flex flex-col gap-2" aria-label={t("pnl.title")}>
            <h2 className="text-xl font-semibold">
              {t("pnl.title")}{" "}
              <span className="text-base font-normal text-muted-foreground">
                {s.current.range.from ? `${s.current.range.from} – ${s.current.range.to}` : t("allTime")}
              </span>
            </h2>
            <Pnl current={s.current} previous={s.previous} />
            {s.current.cogs === null && <p className="text-sm text-muted-foreground">{t("noCosts")}</p>}
            {s.current.unknown_cost_net > 0 && s.current.cogs !== null && (
              <p className="text-sm text-muted-foreground">{t("unknownCostNote")}</p>
            )}
          </section>
          <section className="flex flex-col gap-2" aria-label={t("payments.title")}>
            <h2 className="text-xl font-semibold">{t("payments.title")}</h2>
            {s.current.payments.length === 0 ? (
              <p className="text-muted-foreground">{t("empty")}</p>
            ) : (
              <ul className="text-base">
                {s.current.payments.map((p) => (
                  <li key={p.method} className="flex h-10 items-center gap-2 border-b">
                    <span className="flex-1">
                      {t(`payments.methods.${p.method}`, { defaultValue: p.method })}
                    </span>
                    <span className="text-sm text-muted-foreground">×{p.count}</span>
                    <span className="w-32 text-right font-mono">{formatMoney(p.amount)}</span>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      )}
      <SalesBy period={period} deviceId={deviceId || null} />
      <Voids period={period} deviceId={deviceId || null} />
    </section>
  );
}

export function ReportsScreen() {
  const today = useBusinessToday();
  return today.data ? <Reports today={today.data} /> : null;
}
