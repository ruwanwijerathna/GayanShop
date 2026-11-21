import type { Logo, ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { formatMoney } from "@/lib/money";
import { Barcode128 } from "../Barcode128";
import { RECEIPT_LABELS } from "../labels";
import { formatDateTime, ReceiptContainer } from "./ReceiptContainer";

export interface ReturnExchangeTemplateProps {
  receipt: ReceiptData & {
    exchange_policy?: boolean | string | null;
  };
  logo?: Logo | null;
  logoDataUrl?: string | null;
  storeName?: string;
  showExchangePolicy?: boolean;
}

export function ReturnExchangeTemplate({
  receipt,
  logo,
  logoDataUrl,
  storeName,
  showExchangePolicy = true,
}: ReturnExchangeTemplateProps) {
  const { date, time } = formatDateTime(receipt.completed_at || receipt.business_date);

  const tenderedTotal = receipt.payments.reduce((acc, p) => {
    return acc + (p.tendered && p.tendered > p.amount ? p.tendered : p.amount);
  }, 0);

  const netDue = Math.max(0, receipt.grand_total - receipt.exchange_credit);
  const distinctItemCount = receipt.lines.length;

  const isPolicyVisible = showExchangePolicy && receipt.exchange_policy !== false;
  const policyText =
    typeof receipt.exchange_policy === "string" && receipt.exchange_policy.trim().length > 0
      ? receipt.exchange_policy
      : RECEIPT_LABELS.EXCHANGE_POLICY;

  return (
    <ReceiptContainer logo={logo} logoDataUrl={logoDataUrl} storeName={storeName || receipt.store_name}>
      {/* Banner */}
      <div className="border border-black py-1 text-center">
        <p className="font-bold text-xs">{RECEIPT_LABELS.EXCHANGE_TITLE}</p>
        <p className="text-[10px] font-sans uppercase tracking-wider font-semibold">
          {RECEIPT_LABELS.EXCHANGE_SUBTITLE}
        </p>
      </div>

      {/* Bill details */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-xs">
        <div className="flex justify-between gap-1">
          <span className="font-semibold">{RECEIPT_LABELS.BILL_NO} :</span>
          <span className="font-mono font-bold">{receipt.receipt_no}</span>
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
          <span className="font-semibold">{RECEIPT_LABELS.CASHIER} :</span>
          <span className="font-medium">{receipt.cashier}</span>
        </div>
        {receipt.customer_masked && (
          <div className="flex justify-between gap-1">
            <span className="font-semibold">{RECEIPT_LABELS.CUSTOMER} :</span>
            <span className="font-mono">{receipt.customer_masked}</span>
          </div>
        )}
      </div>

      {/* 1. New Items Section */}
      {receipt.lines.length > 0 && (
        <div className="flex flex-col gap-1 border-t border-dashed border-black/80 pt-1.5 text-xs">
          <p className="font-bold text-[11px] underline">{RECEIPT_LABELS.NEW_ITEMS}</p>
          {receipt.lines.map((line, idx) => (
            <div key={idx} className="flex flex-col">
              <span className="font-medium truncate">
                <span className="font-mono font-bold mr-1">{idx + 1}.</span>
                {line.name}
              </span>
              <div className="flex justify-between pl-3 text-neutral-800">
                <span className="font-mono">
                  {line.qty} x {formatMoney(line.unit_price).replace("Rs ", "")}
                </span>
                <span className="font-mono font-medium">
                  {formatMoney(line.line_gross).replace("Rs ", "")}
                </span>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* 2. Returned Items Section */}
      {receipt.returns.length > 0 && (
        <div className="flex flex-col gap-1 border-t border-dashed border-black/80 pt-1.5 text-xs">
          <p className="font-bold text-[11px] underline">{RECEIPT_LABELS.RETURNED_ITEMS}</p>
          {receipt.returns.map((ret, idx) => (
            <div key={idx} className="flex flex-col">
              <span className="font-medium truncate">
                <span className="font-mono font-bold mr-1">{idx + 1}.</span>
                {ret.name}
              </span>
              <div className="flex justify-between pl-3 text-neutral-800">
                <span className="font-mono text-[11px]">
                  {ret.qty} x (බිල්: {ret.original_receipt_no})
                </span>
                <span className="font-mono font-medium text-destructive">
                  -{formatMoney(ret.value).replace("Rs ", "")}
                </span>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Totals */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-xs">
        <div className="flex justify-between">
          <span className="font-medium">{RECEIPT_LABELS.ITEMS_COUNT} :</span>
          <span className="font-mono font-bold">{distinctItemCount}</span>
        </div>
        <div className="flex justify-between">
          <span className="font-medium font-sans">{RECEIPT_LABELS.GROSS_TOTAL}</span>
          <span className="font-mono">{formatMoney(receipt.gross_sales).replace("Rs ", "")}</span>
        </div>
        {receipt.exchange_credit > 0 && (
          <div className="flex justify-between text-neutral-800">
            <span className="font-medium">{RECEIPT_LABELS.EXCHANGE_CREDIT} :</span>
            <span className="font-mono font-semibold">
              -{formatMoney(receipt.exchange_credit).replace("Rs ", "")}
            </span>
          </div>
        )}
        <div className="my-0.5 flex justify-between border-y border-black py-1 text-sm font-bold">
          <span>{RECEIPT_LABELS.AMOUNT_DUE} :</span>
          <span className="font-mono">{formatMoney(netDue)}</span>
        </div>
        <div className="flex justify-between">
          <span>{RECEIPT_LABELS.PAID} :</span>
          <span className="font-mono font-medium">{formatMoney(tenderedTotal)}</span>
        </div>
        <div className="flex justify-between">
          <span>{RECEIPT_LABELS.CHANGE} :</span>
          <span className="font-mono font-medium">{formatMoney(receipt.change_due)}</span>
        </div>
      </div>

      {/* 7-day policy & Thank you note */}
      <div className="flex flex-col items-center gap-1 border-t border-dashed border-black/80 pt-2 text-center text-xs">
        {isPolicyVisible && <p className="px-1 text-[11px] leading-snug">{policyText}</p>}
        <p className="font-bold text-sm">{RECEIPT_LABELS.THANK_YOU}</p>
      </div>

      {/* Barcode128 */}
      <div className="flex justify-center pt-1">
        <Barcode128 data={receipt.receipt_no} height={42} />
      </div>

      {/* Footer Branding */}
      <div className="flex flex-col items-center gap-0.5 pt-1 text-center text-[11px] text-neutral-700 font-sans">
        <p className="font-medium">{RECEIPT_LABELS.POWERED_BY}</p>
      </div>
    </ReceiptContainer>
  );
}
