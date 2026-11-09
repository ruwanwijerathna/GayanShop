// Back-office dashboard (doc §1.4, REPORT_SALES): today's key figures against yesterday, the last 30 days
// against the 30 before, today's payment methods, this year by month, and — with CUSTOMER_VIEW — this
// month's top customers. Everything is pre-aggregated in Rust; costs only appear with the cost permissions.
import { useTranslation } from "react-i18next";
import { Can } from "@/components/auth/Can";
import { PaymentDonut } from "@/components/charts/PaymentDonut";
import { RevenueExpenseBar } from "@/components/charts/RevenueExpenseBar";
import { SalesTrendArea } from "@/components/charts/SalesTrendArea";
import { TopCustomersBar } from "@/components/charts/TopCustomersBar";
import { percentBp } from "@/components/charts/format";
import type { FinancialSummary_Serialize as FinancialSummary, Period } from "@/db/bindings";
import { useBusinessToday, useCustomerAnalytics, useReportSummary, useReportTrend } from "@/db/hooks";
import { formatMoney } from "@/lib/money";
import { cn } from "@/lib/utils";

/** `YYYY-MM-DD` ± n days (UTC arithmetic on a calendar date: no time zone involved). */
export function addDays(date: string, n: number): string {
  const d = new Date(`${date}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + n);
  return d.toISOString().slice(0, 10);
}

type Kpi = { key: "net_sales" | "sales_count" | "avg_basket" | "gross_profit"; money: boolean };
const KPIS: Kpi[] = [
  { key: "net_sales", money: true },
  { key: "sales_count", money: false },
  { key: "avg_basket", money: true },
  { key: "gross_profit", money: true },
];

function KpiCard({
  kpi,
  current,
  previous,
}: {
  kpi: Kpi;
  current: FinancialSummary;
  previous: FinancialSummary | null;
}) {
  const { t } = useTranslation("reports");
  const value = current[kpi.key];
  if (value === null) return null;
  const before = previous ? previous[kpi.key] : null;
  const change =
    before !== null && before !== 0 ? Math.trunc(((value - before) * 10_000) / Math.abs(before)) : null;
  return (
    <div className="flex flex-col gap-1 rounded-lg border bg-card p-4">
      <span className="text-sm text-muted-foreground">{t(`dashboard.kpis.${kpi.key}`)}</span>
      <span className="font-mono text-2xl font-semibold">{kpi.money ? formatMoney(value) : value}</span>
      {change !== null && (
        <span className={cn("text-sm", change < 0 ? "text-destructive" : "text-success-foreground")}>
          {change > 0 ? "+" : ""}
          {percentBp(change)} {t("dashboard.vsYesterday")}
        </span>
      )}
    </div>
  );
}

function TopCustomersThisMonth({ month }: { month: Period }) {
  const { t } = useTranslation("customers");
  const a = useCustomerAnalytics(month, "week");
  if (!a.data) return null;
  return (
    <TopCustomersBar
      customers={a.data.top.map((c) => ({
        id: c.customer_id,
        label: c.display_name ?? c.phone_display ?? t("erased"),
        visits: c.visits,
        spend: c.spend,
      }))}
    />
  );
}

function Board({ today }: { today: string }) {
  const { t } = useTranslation("reports");
  const day: Period = { kind: "day", date: today };
  const last30: Period = { kind: "custom", from: addDays(today, -29), to: today };
  const year: Period = { kind: "year", year: Number.parseInt(today.slice(0, 4), 10) };
  const month: Period = {
    kind: "month",
    year: Number.parseInt(today.slice(0, 4), 10),
    month: Number.parseInt(today.slice(5, 7), 10),
  };
  const summary = useReportSummary(day, null, true);
  const trend = useReportTrend(last30, null, "day", true);
  const byMonth = useReportTrend(year, null, "month", false);
  const s = summary.data;
  return (
    <section className="flex flex-col gap-6" aria-label={t("dashboard.title")}>
      <h2 className="text-2xl font-semibold">{t("dashboard.title")}</h2>
      {s && (
        <div className="grid grid-cols-2 gap-4 xl:grid-cols-4">
          {KPIS.map((k) => (
            <KpiCard key={k.key} kpi={k} current={s.current} previous={s.previous} />
          ))}
        </div>
      )}
      <div className="grid gap-6 xl:grid-cols-2">
        {trend.data && (
          <div className="flex flex-col gap-1">
            <span className="text-sm text-muted-foreground">{t("dashboard.last30")}</span>
            <SalesTrendArea current={trend.data.current} previous={trend.data.previous} />
          </div>
        )}
        {s && <PaymentDonut payments={s.current.payments} />}
        {byMonth.data && (
          <div className="flex flex-col gap-1">
            <span className="text-sm text-muted-foreground">{t("dashboard.thisYear")}</span>
            <RevenueExpenseBar points={byMonth.data.current} />
          </div>
        )}
        <Can perm="CUSTOMER_VIEW">
          <div className="flex flex-col gap-1">
            <span className="text-sm text-muted-foreground">{t("dashboard.thisMonth")}</span>
            <TopCustomersThisMonth month={month} />
          </div>
        </Can>
      </div>
    </section>
  );
}

export function Dashboard() {
  const today = useBusinessToday();
  return today.data ? <Board today={today.data} /> : null;
}
