import type { Logo, ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { formatMoney } from "@/lib/money";
import { FM_LABELS, toFm } from "@/lib/unicodeToFm";
import { Barcode128 } from "../Barcode128";
import { formatDateTime, ReceiptContainer } from "./ReceiptContainer";

export interface ReturnExchangeTemplateProps {
  receipt: ReceiptData;
  logo?: Logo | null;
  logoDataUrl?: string | null;
  storeName?: string;
}

export function ReturnExchangeTemplate({
  receipt,
  logo,
  logoDataUrl,
  storeName,
}: ReturnExchangeTemplateProps) {
  const { date, time } = formatDateTime(receipt.completed_at || receipt.business_date);

  const tenderedTotal = receipt.payments.reduce((acc, p) => {
    return acc + (p.tendered && p.tendered > p.amount ? p.tendered : p.amount);
  }, 0);

  const netDue = Math.max(0, receipt.grand_total - receipt.exchange_credit);

  return (
    <ReceiptContainer logo={logo} logoDataUrl={logoDataUrl} storeName={storeName || receipt.store_name}>
      {/* Banner */}
      <div className="border border-black py-1 text-center">
        <p className="font-bold text-xs">{FM_LABELS.EXCHANGE_TITLE}</p>
        <p className="text-[10px] font-sans uppercase tracking-wider font-semibold">
          (EXCHANGE / RETURN SLIP)
        </p>
      </div>

      {/* Bill details */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-xs">
        <div className="flex justify-between gap-1">
          <span className="font-semibold">{FM_LABELS.BILL_NO} :</span>
          <span className="font-mono font-bold">{receipt.receipt_no}</span>
        </div>
        <div className="flex justify-between gap-1">
          <span>
            <span className="font-semibold">{FM_LABELS.DATE} : </span>
            <span className="font-mono">{date}</span>
          </span>
          <span>
            <span className="font-semibold">{FM_LABELS.TIME} : </span>
            <span className="font-mono">{time}</span>
          </span>
        </div>
        <div className="flex justify-between gap-1">
          <span className="font-semibold">{FM_LABELS.CASHIER} :</span>
          <span className="font-medium">{toFm(receipt.cashier)}</span>
        </div>
        {receipt.customer_masked && (
          <div className="flex justify-between gap-1">
            <span className="font-semibold">{FM_LABELS.CUSTOMER} :</span>
            <span className="font-mono">{receipt.customer_masked}</span>
          </div>
        )}
      </div>

      {/* 1. New Items Section */}
      {receipt.lines.length > 0 && (
        <div className="flex flex-col gap-1 border-t border-dashed border-black/80 pt-1.5 text-xs">
          <p className="font-bold text-[11px] underline">{FM_LABELS.NEW_ITEMS}</p>
          {receipt.lines.map((line, idx) => (
            <div key={idx} className="flex flex-col">
              <span className="font-medium truncate">{toFm(line.name)}</span>
              <div className="flex justify-between text-neutral-800">
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
          <p className="font-bold text-[11px] underline">{FM_LABELS.RETURNED_ITEMS}</p>
          {receipt.returns.map((ret, idx) => (
            <div key={idx} className="flex flex-col">
              <span className="font-medium truncate">{toFm(ret.name)}</span>
              <div className="flex justify-between text-neutral-800">
                <span className="font-mono text-[11px]">
                  {ret.qty} x (ì,a: {ret.original_receipt_no})
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
          <span className="font-medium">{FM_LABELS.ITEMS_COUNT} :</span>
          <span className="font-mono font-bold">{receipt.item_count}</span>
        </div>
        <div className="flex justify-between">
          <span className="font-medium font-sans">Gross Total</span>
          <span className="font-mono">{formatMoney(receipt.gross_sales).replace("Rs ", "")}</span>
        </div>
        {receipt.exchange_credit > 0 && (
          <div className="flex justify-between text-neutral-800">
            <span className="font-medium">{FM_LABELS.EXCHANGE_CREDIT} :</span>
            <span className="font-mono font-semibold">
              -{formatMoney(receipt.exchange_credit).replace("Rs ", "")}
            </span>
          </div>
        )}
        <div className="my-0.5 flex justify-between border-y border-black py-1 text-sm font-bold">
          <span>{FM_LABELS.AMOUNT_DUE} :</span>
          <span className="font-mono">{formatMoney(netDue)}</span>
        </div>
        <div className="flex justify-between">
          <span>{FM_LABELS.PAID} :</span>
          <span className="font-mono font-medium">{formatMoney(tenderedTotal)}</span>
        </div>
        <div className="flex justify-between">
          <span>{FM_LABELS.CHANGE} :</span>
          <span className="font-mono font-medium">{formatMoney(receipt.change_due)}</span>
        </div>
      </div>

      {/* 7-day policy & Thank you note */}
      <div className="flex flex-col items-center gap-1 border-t border-dashed border-black/80 pt-2 text-center text-xs">
        <p className="px-1 text-[11px] leading-snug">{FM_LABELS.EXCHANGE_POLICY}</p>
        <p className="font-bold text-sm">{FM_LABELS.THANK_YOU}</p>
      </div>

      {/* Barcode128 */}
      <div className="flex justify-center pt-1">
        <Barcode128 data={receipt.receipt_no} height={42} />
      </div>

      {/* Footer Branding */}
      <div className="flex flex-col items-center gap-0.5 pt-1 text-center text-[11px] text-neutral-700 font-sans">
        <p className="font-medium">{FM_LABELS.POWERED_BY}</p>
      </div>
    </ReceiptContainer>
  );
}
