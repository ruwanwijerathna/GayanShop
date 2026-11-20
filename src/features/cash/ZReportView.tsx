import { useTranslation } from "react-i18next";
import type { ZReport } from "@/db/bindings";
import { formatMoney } from "@/lib/money";
import { cn } from "@/lib/utils";

function Row({ label, value, strong = false }: { label: string; value: string; strong?: boolean }) {
  return (
    <div className={cn("flex justify-between gap-4 py-1", strong && "font-semibold")}>
      <dt>{label}</dt>
      <dd className="font-mono">{value}</dd>
    </div>
  );
}

/** The Z-report (or X-report while the session is open), as printed: one session on one counter. */
export function ZReportView({ report }: { report: ZReport }) {
  const { t } = useTranslation("cash");
  const variance = report.variance;
  return (
    <section aria-labelledby="z-title" className="flex flex-col gap-3 rounded-lg border border-border p-4">
      <h2 id="z-title" className="text-xl font-semibold">
        {t("z.title")}
        <span className="ml-3 text-sm font-normal text-muted-foreground">{report.session.business_date}</span>
      </h2>
      <dl className="divide-y divide-border">
        <Row label={t("z.sales")} value={String(report.sales_count)} />
        <Row label={t("z.items")} value={String(report.items_sold)} />
        <Row label={t("z.gross")} value={formatMoney(report.gross_sales)} />
        <Row label={t("z.discounts")} value={formatMoney(report.discount_total)} />
        <Row label={t("z.total")} value={formatMoney(report.sales_total)} strong />
      </dl>
      <h3 className="font-semibold">{t("z.payments")}</h3>
      <dl className="divide-y divide-border">
        {report.payments.map((p) => (
          <Row
            key={p.method}
            label={t(`z.method.${p.method}`, { defaultValue: p.method })}
            value={`${p.count} · ${formatMoney(p.amount)}`}
          />
        ))}
        <Row label={t("z.change")} value={formatMoney(report.change_given)} />
        <Row
          label={t("z.overrides")}
          value={`${report.overrides_count} · ${formatMoney(report.overrides_value)}`}
        />
        <Row
          label={t("z.quickSales")}
          value={`${report.quick_sale_lines} · ${formatMoney(report.quick_sale_net)}`}
        />
        {report.exchanges_count > 0 && (
          <>
            <Row
              label={t("z.exchanges")}
              value={`${report.exchanges_count} · ${formatMoney(report.exchange_credit)}`}
            />
            <Row label={t("z.returnedItems")} value={String(report.returned_items)} />
          </>
        )}
        <Row label={t("z.voids")} value={`${report.voids_count} · ${formatMoney(report.voids_value)}`} />
      </dl>
      {report.voids.length > 0 && (
        <ul className="flex flex-col gap-1 text-sm">
          {report.voids.map((v) => (
            <li key={`${v.receipt_no}-${v.voided_at}`}>
              <span className="font-mono">{v.receipt_no}</span> · {t(`z.reason.${v.reason}`)} ·{" "}
              {formatMoney(v.value)} · {v.voided_by}
              {v.approved_by && ` (${t("z.approvedBy", { name: v.approved_by })})`}
            </li>
          ))}
        </ul>
      )}
      <dl className="divide-y divide-border">
        <Row label={t("z.float")} value={formatMoney(report.opening_float)} />
        <Row label={t("z.cashSales")} value={formatMoney(report.cash_sales)} />
        <Row label={t("z.payIns")} value={formatMoney(report.pay_ins)} />
        <Row label={t("z.payOuts")} value={formatMoney(report.pay_outs)} />
        <Row label={t("z.noSales")} value={String(report.no_sales)} />
        <Row label={t("z.expected")} value={formatMoney(report.expected_cash)} strong />
        {report.counted_cash !== null && report.counted_cash !== undefined && (
          <Row label={t("z.counted")} value={formatMoney(report.counted_cash)} strong />
        )}
        {variance !== null && variance !== undefined && (
          <div
            className={cn(
              "flex justify-between gap-4 py-1 font-semibold",
              variance < 0 ? "text-destructive" : variance === 0 && "text-success-foreground",
            )}
          >
            <dt>{t("z.variance")}</dt>
            <dd className="font-mono">{formatMoney(variance)}</dd>
          </div>
        )}
      </dl>
    </section>
  );
}
