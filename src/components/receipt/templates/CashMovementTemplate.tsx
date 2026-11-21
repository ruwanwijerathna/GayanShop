import type { Logo } from "@/db/bindings";
import { formatMoney } from "@/lib/money";
import { FM_LABELS, toFm } from "@/lib/unicodeToFm";
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
        <p className="font-bold text-xs">{FM_LABELS.DRAWER_SLIP_TITLE}</p>
        <p className="text-[11px] font-bold tracking-wide">
          {isPayIn
            ? FM_LABELS.PAY_IN_TITLE
            : isPayOut
              ? FM_LABELS.PAY_OUT_TITLE
              : "uqo,a ,dÉpqj újD; lsÍu (NO-SALE OPEN)"}
        </p>
      </div>

      {/* Details */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-xs">
        <div className="flex justify-between">
          <span className="font-semibold">{FM_LABELS.DATE} : </span>
          <span className="font-mono">
            {date} {time}
          </span>
        </div>
        <div className="flex justify-between">
          <span className="font-semibold">{FM_LABELS.CASHIER} :</span>
          <span className="font-medium">{toFm(data.staff)}</span>
        </div>
        {data.approved_by && (
          <div className="flex justify-between">
            <span className="font-semibold">{FM_LABELS.SIGNATURE_APPROVED} :</span>
            <span className="font-medium">{toFm(data.approved_by)}</span>
          </div>
        )}
      </div>

      {/* Amount Box */}
      <div className="my-1 rounded border-2 border-black p-2 text-center">
        <p className="text-xs font-semibold">{FM_LABELS.AMOUNT} :</p>
        <p className="font-mono text-base font-bold tracking-tight">{formatMoney(data.amount)}</p>
      </div>

      {/* Reason */}
      <div className="flex flex-col gap-0.5 text-xs">
        <span className="font-semibold">{FM_LABELS.REASON} :</span>
        <p className="rounded bg-neutral-50 p-1 font-sans text-neutral-800 border border-neutral-300">
          {toFm(data.reason)}
        </p>
      </div>

      {/* Signature Lines */}
      <div className="mt-4 flex justify-between gap-4 pt-4 text-center text-[10px]">
        <div className="flex-1 border-t border-black pt-1">
          <p>{FM_LABELS.SIGNATURE_CASHIER}</p>
        </div>
        <div className="flex-1 border-t border-black pt-1">
          <p>{FM_LABELS.SIGNATURE_APPROVED}</p>
        </div>
      </div>

      {/* Footer Branding */}
      <div className="flex flex-col items-center gap-0.5 border-t border-dashed border-black/80 pt-2 text-center text-[11px] text-neutral-700 font-sans">
        <p className="font-medium">{FM_LABELS.POWERED_BY}</p>
      </div>
    </ReceiptContainer>
  );
}
