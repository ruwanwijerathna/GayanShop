import type { Logo } from "@/db/bindings";
import { formatMoney } from "@/lib/money";
import { RECEIPT_LABELS } from "../labels";
import type { CashMovementPrintData } from "../types";
import { formatDateTime, ReceiptContainer } from "./ReceiptContainer";

export interface CashMovementTemplateProps {
  data: CashMovementPrintData;
  logo?: Logo | null;
  logoDataUrl?: string | null;
  storeName?: string;
}

export function CashMovementTemplate({ data, logo, logoDataUrl, storeName }: CashMovementTemplateProps) {
  const { date, time } = formatDateTime(data.created_at);

  const isPayIn = data.kind === "pay_in";
  const isPayOut = data.kind === "pay_out";

  return (
    <ReceiptContainer logo={logo} logoDataUrl={logoDataUrl} storeName={storeName || data.store_name}>
      {/* Title */}
      <div className="border border-black py-1 text-center">
        <p className="font-bold text-xs">{RECEIPT_LABELS.DRAWER_SLIP_TITLE}</p>
        <p className="text-[11px] font-bold tracking-wide">
          {isPayIn
            ? RECEIPT_LABELS.PAY_IN_TITLE
            : isPayOut
              ? RECEIPT_LABELS.PAY_OUT_TITLE
              : RECEIPT_LABELS.NO_SALE_TITLE}
        </p>
      </div>

      {/* Details */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-xs">
        <div className="flex justify-between">
          <span className="font-semibold">{RECEIPT_LABELS.DATE} : </span>
          <span className="font-mono">
            {date} {time}
          </span>
        </div>
        <div className="flex justify-between">
          <span className="font-semibold">{RECEIPT_LABELS.CASHIER} :</span>
          <span className="font-medium">{data.staff}</span>
        </div>
        {data.approved_by && (
          <div className="flex justify-between">
            <span className="font-semibold">{RECEIPT_LABELS.SIGNATURE_APPROVED} :</span>
            <span className="font-medium">{data.approved_by}</span>
          </div>
        )}
      </div>

      {/* Amount Box */}
      <div className="my-1 rounded border-2 border-black p-2 text-center">
        <p className="text-xs font-semibold">{RECEIPT_LABELS.AMOUNT} :</p>
        <p className="font-mono text-base font-bold tracking-tight">{formatMoney(data.amount)}</p>
      </div>

      {/* Reason */}
      <div className="flex flex-col gap-0.5 text-xs">
        <span className="font-semibold">{RECEIPT_LABELS.REASON} :</span>
        <p className="rounded bg-neutral-50 p-1 font-sans text-neutral-800 border border-neutral-300">
          {data.reason}
        </p>
      </div>

      {/* Signature Lines */}
      <div className="mt-4 flex justify-between gap-4 pt-4 text-center text-[10px]">
        <div className="flex-1 border-t border-black pt-1">
          <p>{RECEIPT_LABELS.SIGNATURE_CASHIER}</p>
        </div>
        <div className="flex-1 border-t border-black pt-1">
          <p>{RECEIPT_LABELS.SIGNATURE_APPROVED}</p>
        </div>
      </div>

      {/* Footer Branding */}
      <div className="flex flex-col items-center gap-0.5 border-t border-dashed border-black/80 pt-2 text-center text-[11px] text-neutral-700 font-sans">
        <p className="font-medium">{RECEIPT_LABELS.POWERED_BY}</p>
      </div>
    </ReceiptContainer>
  );
}
