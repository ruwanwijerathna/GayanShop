import type { Logo, ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { formatMoney } from "@/lib/money";
import { Barcode128 } from "../Barcode128";
import { RECEIPT_LABELS } from "../labels";
import { formatDateTime, ReceiptContainer } from "./ReceiptContainer";

export interface SaleReceiptTemplateProps {
  receipt: ReceiptData;
  logo?: Logo | null;
  logoDataUrl?: string | null;
  storeName?: string;
}

export function SaleReceiptTemplate({ receipt, logo, logoDataUrl, storeName }: SaleReceiptTemplateProps) {
  const { date, time } = formatDateTime(receipt.completed_at || receipt.business_date);

  const tenderedTotal = receipt.payments.reduce((acc, p) => {
    return acc + (p.tendered && p.tendered > p.amount ? p.tendered : p.amount);
  }, 0);

  const isSample = receipt.order_id === "SAMPLE-RECEIPT" || receipt.order_id.includes("SAMPLE");

  return (
    <ReceiptContainer logo={logo} logoDataUrl={logoDataUrl} storeName={storeName || receipt.store_name}>
      {isSample && (
        <p className="text-center text-xs font-bold font-mono tracking-wide">
          {RECEIPT_LABELS.SAMPLE_RECEIPT}
        </p>
      )}

      {receipt.status === "voided" && (
        <div className="border border-black py-0.5 text-center text-xs font-bold uppercase tracking-wider">
          {RECEIPT_LABELS.VOIDED_RECEIPT}
        </div>
      )}

      {/* Bill details: බිල් අංකය, දිනය, වේලාව, අයකැමි */}
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

      {/* Items List */}
      <div className="flex flex-col gap-1 border-t border-dashed border-black/80 pt-1.5 text-xs">
        {receipt.lines.map((line, idx) => (
          <div key={idx} className="flex flex-col">
            <div className="flex justify-between font-medium">
              <span className="truncate pr-1">{line.name}</span>
            </div>
            <div className="flex justify-between text-neutral-800">
              <span className="font-mono">
                {line.qty} x {formatMoney(line.unit_price).replace("Rs ", "")}
              </span>
              <span className="font-mono font-medium">{formatMoney(line.line_gross).replace("Rs ", "")}</span>
            </div>
            {line.line_discount > 0 && (
              <div className="flex justify-between text-[11px] text-neutral-700 italic">
                <span>{RECEIPT_LABELS.DISCOUNT}</span>
                <span className="font-mono">-{formatMoney(line.line_discount).replace("Rs ", "")}</span>
              </div>
            )}
          </div>
        ))}
      </div>

      {/* Totals: භාණ්ඩ ගණන, Subtotal, TOTAL, Cash, Change */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-xs">
        <div className="flex justify-between">
          <span className="font-semibold">{RECEIPT_LABELS.ITEMS_COUNT} :</span>
          <span className="font-mono font-bold">{receipt.item_count}</span>
        </div>
        <div className="flex justify-between">
          <span className="font-medium font-sans">{RECEIPT_LABELS.SUBTOTAL}</span>
          <span className="font-mono">{formatMoney(receipt.gross_sales).replace("Rs ", "")}</span>
        </div>
        {receipt.discount_total > 0 && (
          <div className="flex justify-between text-neutral-800">
            <span>{RECEIPT_LABELS.DISCOUNT}</span>
            <span className="font-mono">-{formatMoney(receipt.discount_total).replace("Rs ", "")}</span>
          </div>
        )}
        <div className="my-0.5 flex justify-between border-y border-black py-1 text-sm font-bold">
          <span>{RECEIPT_LABELS.TOTAL}</span>
          <span className="font-mono">{formatMoney(receipt.grand_total)}</span>
        </div>
        <div className="flex justify-between">
          <span>{RECEIPT_LABELS.PAID}</span>
          <span className="font-mono font-medium">{formatMoney(tenderedTotal).replace("Rs ", "")}</span>
        </div>
        <div className="flex justify-between">
          <span>{RECEIPT_LABELS.CHANGE}</span>
          <span className="font-mono font-medium">{formatMoney(receipt.change_due).replace("Rs ", "")}</span>
        </div>
      </div>

      {/* 7-day policy & Thank you note */}
      <div className="flex flex-col items-center gap-1 border-t border-dashed border-black/80 pt-2 text-center text-xs">
        <p className="px-1 text-[11px] leading-snug">{RECEIPT_LABELS.EXCHANGE_POLICY}</p>
        <p className="font-bold text-sm">{RECEIPT_LABELS.THANK_YOU}</p>
      </div>

      {/* Barcode128 */}
      <div className="flex justify-center pt-1">
        <Barcode128 data={receipt.receipt_no} height={42} />
      </div>

      {/* Footer Branding */}
      <div className="flex flex-col items-center gap-0.5 pt-1 text-center text-[11px] text-neutral-700 font-sans">
        <p className="font-medium">{RECEIPT_LABELS.POWERED_BY}</p>
        {receipt.footer && (
          <p className="whitespace-pre-wrap text-[10px] text-neutral-500">{receipt.footer}</p>
        )}
      </div>
    </ReceiptContainer>
  );
}
