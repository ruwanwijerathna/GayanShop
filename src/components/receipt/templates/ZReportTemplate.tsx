import type { Logo, ZReport_Serialize as ZReportData } from "@/db/bindings";
import { formatMoney } from "@/lib/money";
import { RECEIPT_LABELS } from "../labels";
import { formatDateTime, ReceiptContainer } from "./ReceiptContainer";

export interface ZReportTemplateProps {
  report: ZReportData;
  logo?: Logo | null;
  logoDataUrl?: string | null;
  storeName?: string;
  isReprint?: boolean;
}

export function ZReportTemplate({
  report,
  logo,
  logoDataUrl,
  storeName,
  isReprint = false,
}: ZReportTemplateProps) {
  const { session } = report;
  const opened = formatDateTime(session.opened_at);
  const closed = session.closed_at ? formatDateTime(session.closed_at) : null;

  return (
    <ReceiptContainer logo={logo} logoDataUrl={logoDataUrl} storeName={storeName}>
      {/* Title */}
      <div className="border border-black py-1 text-center">
        <p className="font-bold text-xs">{RECEIPT_LABELS.Z_REPORT_TITLE}</p>
        {isReprint && <p className="font-mono text-xs font-bold text-destructive">*** REPRINT ***</p>}
      </div>

      {/* Session Details */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-xs">
        <div className="flex justify-between">
          <span className="font-semibold">{RECEIPT_LABELS.BUSINESS_DATE} :</span>
          <span className="font-mono font-bold">{session.business_date}</span>
        </div>
        <div className="flex justify-between">
          <span>{RECEIPT_LABELS.OPENED} :</span>
          <span className="font-mono">
            {opened.date} {opened.time}
          </span>
        </div>
        <div className="flex justify-between">
          <span>{RECEIPT_LABELS.OPENED_BY} :</span>
          <span className="font-medium">{session.opened_by_name}</span>
        </div>
        {closed && (
          <>
            <div className="flex justify-between">
              <span>{RECEIPT_LABELS.CLOSED} :</span>
              <span className="font-mono">
                {closed.date} {closed.time}
              </span>
            </div>
            <div className="flex justify-between">
              <span>{RECEIPT_LABELS.CLOSED_BY} :</span>
              <span className="font-medium">{session.closed_by_name ?? ""}</span>
            </div>
          </>
        )}
      </div>

      {/* Sales Summary */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-xs">
        <p className="font-bold text-[11px] underline">{RECEIPT_LABELS.SALES_SUMMARY}</p>
        <div className="flex justify-between">
          <span>{RECEIPT_LABELS.SALES_COUNT} :</span>
          <span className="font-mono font-bold">{report.sales_count}</span>
        </div>
        <div className="flex justify-between">
          <span>{RECEIPT_LABELS.ITEMS_SOLD} :</span>
          <span className="font-mono">{report.items_sold}</span>
        </div>
        <div className="flex justify-between">
          <span>{RECEIPT_LABELS.GROSS_SALES} :</span>
          <span className="font-mono">{formatMoney(report.gross_sales).replace("Rs ", "")}</span>
        </div>
        {report.discount_total > 0 && (
          <div className="flex justify-between text-neutral-800">
            <span>{RECEIPT_LABELS.DISCOUNT} :</span>
            <span className="font-mono">-{formatMoney(report.discount_total).replace("Rs ", "")}</span>
          </div>
        )}
        <div className="my-0.5 flex justify-between border-y border-black py-0.5 font-bold">
          <span>{RECEIPT_LABELS.NET_SALES} :</span>
          <span className="font-mono">{formatMoney(report.sales_total)}</span>
        </div>
      </div>

      {/* Payments */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-xs">
        <p className="font-bold text-[11px] underline">{RECEIPT_LABELS.PAYMENTS}</p>
        {report.payments.map((p, idx) => (
          <div key={idx} className="flex justify-between">
            <span className="capitalize">
              {p.method} ({p.count})
            </span>
            <span className="font-mono">{formatMoney(p.amount).replace("Rs ", "")}</span>
          </div>
        ))}
        {report.change_given > 0 && (
          <div className="flex justify-between text-neutral-800">
            <span>ඉතිරි මුදල් (Change) :</span>
            <span className="font-mono">{formatMoney(report.change_given).replace("Rs ", "")}</span>
          </div>
        )}
      </div>

      {/* Drawer Summary */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-xs">
        <p className="font-bold text-[11px] underline">{RECEIPT_LABELS.DRAWER_SUMMARY}</p>
        <div className="flex justify-between">
          <span>{RECEIPT_LABELS.OPENING_FLOAT} :</span>
          <span className="font-mono">{formatMoney(session.opening_float).replace("Rs ", "")}</span>
        </div>
        <div className="flex justify-between">
          <span>{RECEIPT_LABELS.CASH_SALES} :</span>
          <span className="font-mono">{formatMoney(report.cash_sales).replace("Rs ", "")}</span>
        </div>
        {report.pay_ins > 0 && (
          <div className="flex justify-between">
            <span>{RECEIPT_LABELS.PAY_INS} :</span>
            <span className="font-mono">{formatMoney(report.pay_ins).replace("Rs ", "")}</span>
          </div>
        )}
        {report.pay_outs > 0 && (
          <div className="flex justify-between text-destructive">
            <span>{RECEIPT_LABELS.PAY_OUTS} :</span>
            <span className="font-mono">-{formatMoney(report.pay_outs).replace("Rs ", "")}</span>
          </div>
        )}
        <div className="my-0.5 flex justify-between border-t border-black pt-0.5 font-bold">
          <span>{RECEIPT_LABELS.EXPECTED_CASH} :</span>
          <span className="font-mono">
            {session.expected_cash != null ? formatMoney(session.expected_cash) : "-"}
          </span>
        </div>
        {session.counted_cash != null && (
          <div className="flex justify-between font-medium">
            <span>{RECEIPT_LABELS.COUNTED_CASH} :</span>
            <span className="font-mono">{formatMoney(session.counted_cash)}</span>
          </div>
        )}
        {session.variance != null && (
          <div className="flex justify-between border-t border-dashed border-black/80 pt-0.5 font-bold">
            <span>{RECEIPT_LABELS.VARIANCE} :</span>
            <span className="font-mono">
              {session.variance > 0 ? `+${formatMoney(session.variance)}` : formatMoney(session.variance)}
            </span>
          </div>
        )}
      </div>

      {/* Footer Branding */}
      <div className="flex flex-col items-center gap-0.5 border-t border-dashed border-black/80 pt-2 text-center text-[11px] text-neutral-700 font-sans">
        <p className="font-medium">{RECEIPT_LABELS.POWERED_BY}</p>
      </div>
    </ReceiptContainer>
  );
}
