import type { Logo } from "@/db/bindings";
import { formatMoney } from "@/lib/money";
import { RECEIPT_LABELS } from "../labels";
import type { EstimatePrintData } from "../types";
import { formatDateTime, ReceiptContainer } from "./ReceiptContainer";

export interface EstimateTemplateProps {
  data: EstimatePrintData;
  logo?: Logo | null;
  logoDataUrl?: string | null;
  storeName?: string;
}

export function EstimateTemplate({ data, logo, logoDataUrl, storeName }: EstimateTemplateProps) {
  const { date, time } = formatDateTime(data.created_at);

  return (
    <ReceiptContainer logo={logo} logoDataUrl={logoDataUrl} storeName={storeName || data.store_name}>
      {/* Title */}
      <div className="border border-black py-1 text-center">
        <p className="font-bold text-xs">{RECEIPT_LABELS.ESTIMATE_TITLE}</p>
      </div>

      <p className="text-center text-[11px] font-semibold text-neutral-700">
        {RECEIPT_LABELS.NOT_A_SALE_NOTE}
      </p>

      {/* Details */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-xs">
        <div className="flex justify-between gap-1">
          <span className="font-semibold">{RECEIPT_LABELS.ESTIMATE_NO} :</span>
          <span className="font-mono font-bold">{data.estimate_no}</span>
        </div>
        <div className="flex justify-between gap-1">
          <span>
            <span className="font-semibold">{RECEIPT_LABELS.DATE} : </span>
            <span className="font-mono">{date}</span>
          </span>
          <span>
            <span className="font-semibold">{RECEIPT_LABELS.TIME} : </span>
            <span className="font-mono">{time}</span>
          </span>
        </div>
        <div className="flex justify-between gap-1">
          <span className="font-semibold">{RECEIPT_LABELS.PREPARED_BY} :</span>
          <span className="font-medium">{data.prepared_by}</span>
        </div>
        {(data.customer_name || data.customer_phone) && (
          <div className="flex justify-between gap-1">
            <span className="font-semibold">{RECEIPT_LABELS.CUSTOMER} :</span>
            <span className="font-medium">
              {[data.customer_name, data.customer_phone].filter(Boolean).join(" - ")}
            </span>
          </div>
        )}
      </div>

      {/* Items list */}
      <div className="flex flex-col gap-1 border-t border-dashed border-black/80 pt-1.5 text-xs">
        {data.lines.map((line, idx) => (
          <div key={idx} className="flex flex-col">
            <span className="font-medium truncate">{line.name}</span>
            <div className="flex justify-between text-neutral-800">
              <span className="font-mono">
                {line.qty} x {formatMoney(line.unit_price).replace("Rs ", "")}
              </span>
              <span className="font-mono font-medium">{formatMoney(line.total).replace("Rs ", "")}</span>
            </div>
          </div>
        ))}
      </div>

      {/* Totals */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-xs">
        <div className="flex justify-between">
          <span className="font-semibold">{RECEIPT_LABELS.ITEMS_COUNT} :</span>
          <span className="font-mono font-bold">{data.item_count}</span>
        </div>
        <div className="my-0.5 flex justify-between border-y border-black py-1 text-sm font-bold">
          <span>{RECEIPT_LABELS.TOTAL} :</span>
          <span className="font-mono">{formatMoney(data.total)}</span>
        </div>
      </div>

      {/* Validity note */}
      <div className="flex flex-col items-center gap-1 border-t border-dashed border-black/80 pt-2 text-center text-xs">
        <p className="px-1 text-[11px] leading-snug">{RECEIPT_LABELS.VALIDITY_NOTE}</p>
        <p className="font-bold text-sm">{RECEIPT_LABELS.THANK_YOU}</p>
      </div>

      {/* Footer Branding */}
      <div className="flex flex-col items-center gap-0.5 pt-1 text-center text-[11px] text-neutral-700 font-sans">
        <p className="font-medium">{RECEIPT_LABELS.POWERED_BY}</p>
      </div>
    </ReceiptContainer>
  );
}
