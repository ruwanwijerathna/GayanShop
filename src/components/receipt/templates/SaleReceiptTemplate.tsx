import { QrCode } from "lucide-react";
import type { Logo, PaperWidth, ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { formatMoney } from "@/lib/money";
import { Barcode128 } from "../Barcode128";
import { RECEIPT_LABELS } from "../labels";
import { formatDateTime, ReceiptContainer } from "./ReceiptContainer";

export interface SaleReceiptTemplateProps {
  receipt: ReceiptData & {
    /** Set to false or null to disable the exchange policy note, or pass a custom text */
    exchange_policy?: boolean | string | null;
  };
  logo?: Logo | null;
  logoDataUrl?: string | null;
  storeName?: string;
  showExchangePolicy?: boolean;
  qr?: boolean;
  paperWidth?: PaperWidth;
}

export function SaleReceiptTemplate({
  receipt,
  logo,
  logoDataUrl,
  storeName,
  showExchangePolicy = true,
  qr = false,
  paperWidth,
}: SaleReceiptTemplateProps) {
  const { date, time } = formatDateTime(receipt.completed_at || receipt.business_date);

  const payments = receipt.payments ?? [];
  const lines = receipt.lines ?? [];
  const tenderedTotal = payments.reduce((acc, p) => {
    return acc + (p.tendered && p.tendered > p.amount ? p.tendered : p.amount);
  }, 0);

  const isSample = receipt.order_id === "SAMPLE-RECEIPT" || (receipt.order_id?.includes("SAMPLE") ?? false);

  // Distinct items count: counts how many distinct lines were bought (e.g. 5 of 'ABC' counts as 1 item)
  const distinctItemCount = lines.length;

  // Exchange policy visibility
  const isPolicyVisible = showExchangePolicy && receipt.exchange_policy !== false;
  const policyText =
    typeof receipt.exchange_policy === "string" && receipt.exchange_policy.trim().length > 0
      ? receipt.exchange_policy
      : RECEIPT_LABELS.EXCHANGE_POLICY;

  return (
    <ReceiptContainer
      logo={logo}
      logoDataUrl={logoDataUrl}
      storeName={storeName || receipt.store_name}
      paperWidth={paperWidth}
    >
      {isSample && (
        <p className="text-center text-[10px] font-bold font-mono tracking-wide">
          {RECEIPT_LABELS.SAMPLE_RECEIPT}
        </p>
      )}

      {receipt.status === "voided" && (
        <div className="border border-black py-0.5 text-center text-[10px] font-bold uppercase tracking-wider">
          {RECEIPT_LABELS.VOIDED_RECEIPT}
        </div>
      )}

      {/* Bill details: බිල් අංකය, දිනය, වේලාව, අයකැමි */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-[10px]">
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
      <div className="flex flex-col gap-1 border-t border-dashed border-black/80 pt-1 text-[10px]">
        {receipt.lines.map((line, idx) => (
          <div key={idx} className="flex flex-col">
            <div className="flex justify-between font-medium">
              <span className="truncate pr-1">
                <span className="font-mono font-bold mr-1">{idx + 1}.</span>
                {line.name}
              </span>
            </div>
            <div className="flex justify-between pl-3 text-neutral-800">
              <span className="font-mono">
                {line.qty} x {formatMoney(line.unit_price).replace("Rs ", "")}
              </span>
              <span className="font-mono font-medium">{formatMoney(line.line_gross).replace("Rs ", "")}</span>
            </div>
            {line.line_discount > 0 && (
              <div className="flex justify-between pl-3 text-[9px] text-neutral-700 italic">
                <span>{RECEIPT_LABELS.DISCOUNT}</span>
                <span className="font-mono">-{formatMoney(line.line_discount).replace("Rs ", "")}</span>
              </div>
            )}
          </div>
        ))}
      </div>

      {/* Totals: Items, Subtotal, TOTAL, Cash, Change */}
      <div className="flex flex-col gap-0.5 border-t border-dashed border-black/80 pt-1.5 text-[10px]">
        <div className="flex justify-between">
          <span className="font-semibold">{RECEIPT_LABELS.ITEMS_COUNT} :</span>
          <span className="font-mono font-bold">{distinctItemCount}</span>
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
      <div className="flex flex-col items-center gap-1 border-t border-dashed border-black/80 pt-2 text-center text-[8px]">
        {isPolicyVisible && <p className="px-1 text-[8px] leading-snug">{policyText}</p>}
        <p className="font-bold text-[10px]">{RECEIPT_LABELS.THANK_YOU}</p>
      </div>

      {/* Barcode128 or QR Code */}
      <div className="flex justify-center pt-1">
        {qr ? (
          <QrCode aria-hidden className="mx-auto size-12" strokeWidth={1.5} />
        ) : (
          <Barcode128 data={receipt.receipt_no} height={24} />
        )}
      </div>

      {/* Footer Branding */}
      <div className="flex flex-col items-center gap-0.5 pt-1 text-center text-[8px] text-neutral-700 font-sans">
        <p className="font-medium">{RECEIPT_LABELS.POWERED_BY}</p>
        {receipt.footer && <p className="text-[7.5px] text-neutral-500 font-sans">{receipt.footer}</p>}
      </div>
    </ReceiptContainer>
  );
}
