import type { Logo, ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { canvasFromLogo } from "@/lib/logoBitmap";
import { formatMoney } from "@/lib/money";
import { Barcode128 } from "./Barcode128";

export interface SinhalaReceiptTemplateProps {
  receipt: ReceiptData;
  logo?: Logo | null;
  logoDataUrl?: string | null;
  storeName?: string;
}

function formatDateTime(iso: string) {
  try {
    const d = new Date(iso);
    if (isNaN(d.getTime())) return { date: iso, time: "" };
    const date = d.toLocaleDateString("en-CA"); // YYYY-MM-DD
    const time = d.toLocaleTimeString("en-US", {
      hour: "2-digit",
      minute: "2-digit",
      hour12: true,
    });
    return { date, time };
  } catch {
    return { date: iso, time: "" };
  }
}

export function SinhalaReceiptTemplate({
  receipt,
  logo,
  logoDataUrl: propLogoUrl,
  storeName,
}: SinhalaReceiptTemplateProps) {
  const logoSrc =
    propLogoUrl ??
    (typeof window !== "undefined" ? localStorage.getItem("receipt_logo_source") : null) ??
    (logo ? canvasFromLogo(logo).toDataURL() : null);

  const { date, time } = formatDateTime(receipt.completed_at || receipt.business_date);

  const tenderedTotal = receipt.payments.reduce((acc, p) => {
    return acc + (p.tendered && p.tendered > p.amount ? p.tendered : p.amount);
  }, 0);

  return (
    <div
      className="receipt-container mx-auto flex flex-col gap-2.5 bg-white p-3 text-black selection:bg-none"
      style={{
        width: "72mm",
        maxWidth: "100%",
        fontFamily: "'Noto Sans Sinhala', 'Inter', sans-serif",
        fontSize: "12px",
        lineHeight: "1.4",
      }}
    >
      {/* 1. Header Logo (Store Name is already inside logo). If no logo, show Store Name */}
      {logoSrc ? (
        <div className="flex justify-center pb-1">
          <img
            src={logoSrc}
            alt={storeName || receipt.store_name}
            className="max-h-28 w-auto max-w-full object-contain"
          />
        </div>
      ) : (
        <h1 className="text-center text-base font-bold">{storeName || receipt.store_name}</h1>
      )}

      {receipt.status === "voided" && (
        <div className="border border-black py-0.5 text-center text-xs font-bold uppercase tracking-wider">
          *** අවලංගු කළ බිල්පතකි (VOIDED) ***
        </div>
      )}

      {/* 2. Bill details: බිල් අංකය, දිනය, වේලාව, අයකැමි */}
      <div className="flex flex-col gap-1 border-t border-dashed border-black/80 pt-2 text-xs">
        <div className="flex justify-between gap-1">
          <span className="font-semibold text-neutral-800">බිල් අංකය :</span>
          <span className="font-mono font-bold">{receipt.receipt_no}</span>
        </div>
        <div className="flex justify-between gap-1">
          <span className="font-semibold text-neutral-800">දිනය :</span>
          <span className="font-mono">{date}</span>
        </div>
        <div className="flex justify-between gap-1">
          <span className="font-semibold text-neutral-800">වේලාව :</span>
          <span className="font-mono">{time}</span>
        </div>
        <div className="flex justify-between gap-1">
          <span className="font-semibold text-neutral-800">අයකැමි :</span>
          <span className="font-medium">{receipt.cashier}</span>
        </div>
        {receipt.customer_masked && (
          <div className="flex justify-between gap-1">
            <span className="font-semibold text-neutral-800">පාරිභෝගික :</span>
            <span className="font-mono">{receipt.customer_masked}</span>
          </div>
        )}
      </div>

      {/* 3. Items list: සාමාන්‍ය පරිදි items ටික */}
      <div className="border-t border-dashed border-black/80 pt-2">
        <div className="mb-1 flex justify-between text-xs font-bold">
          <span>භාණ්ඩය</span>
          <span>මුදල (රු.)</span>
        </div>
        <div className="flex flex-col gap-1.5">
          {receipt.lines.map((l, i) => (
            <div key={i} className="flex flex-col text-xs">
              <span className="font-medium text-black">{l.name}</span>
              <div className="flex justify-between text-neutral-700">
                <span className="font-mono">
                  &nbsp;&nbsp;{l.qty} x {formatMoney(l.unit_price).replace("Rs ", "")}
                </span>
                <span className="font-mono font-medium text-black">
                  {formatMoney(l.line_gross).replace("Rs ", "")}
                </span>
              </div>
              {l.line_discount > 0 && (
                <div className="flex justify-between text-[11px] text-neutral-600">
                  <span>&nbsp;&nbsp;වට්ටම් (Discount)</span>
                  <span className="font-mono">-{formatMoney(l.line_discount).replace("Rs ", "")}</span>
                </div>
              )}
            </div>
          ))}
        </div>
      </div>

      {receipt.returns.length > 0 && (
        <div className="border-t border-dashed border-black/80 pt-2">
          <div className="mb-1 text-xs font-bold">ආපසු ලැබුණු භාණ්ඩ (Returned)</div>
          <div className="flex flex-col gap-1.5">
            {receipt.returns.map((x, i) => (
              <div key={i} className="flex flex-col text-xs">
                <span className="font-medium text-black">{x.name}</span>
                <div className="flex justify-between text-neutral-700">
                  <span className="font-mono">
                    &nbsp;&nbsp;{x.qty} · {x.original_receipt_no}
                  </span>
                  <span className="font-mono font-medium text-black">
                    -{formatMoney(x.value).replace("Rs ", "")}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* 4. Totals: මුළු මුදල, ගෙවූ මුදල, ඉතිරි මුදල, භාණ්ඩ ගණන */}
      <div className="flex flex-col gap-1 border-t border-dashed border-black/80 pt-2 text-xs">
        <div className="flex justify-between gap-1">
          <span>භාණ්ඩ ගණන :</span>
          <span className="font-mono font-semibold">{receipt.item_count}</span>
        </div>

        {receipt.discount_total > 0 && (
          <div className="flex justify-between gap-1">
            <span>මුළු වට්ටම :</span>
            <span className="font-mono font-medium">-{formatMoney(receipt.discount_total)}</span>
          </div>
        )}

        <div className="my-1 flex justify-between border-y border-black/80 py-1 text-sm font-bold">
          <span>මුළු මුදල :</span>
          <span className="font-mono text-base">{formatMoney(receipt.grand_total)}</span>
        </div>

        {receipt.exchange_credit > 0 && (
          <div className="flex justify-between gap-1">
            <span>හුවමාරු අගය (Exchange) :</span>
            <span className="font-mono font-medium">-{formatMoney(receipt.exchange_credit)}</span>
          </div>
        )}

        <div className="flex justify-between gap-1">
          <span>ගෙවූ මුදල :</span>
          <span className="font-mono">
            {formatMoney(tenderedTotal > 0 ? tenderedTotal : receipt.grand_total)}
          </span>
        </div>

        {receipt.change_due > 0 && (
          <div className="flex justify-between gap-1">
            <span>ඉතිරි මුදල :</span>
            <span className="font-mono font-semibold">{formatMoney(receipt.change_due)}</span>
          </div>
        )}
      </div>

      {/* 5. Policy note: දින 7ක් ඇතුලත බිල්පත සමග භාණ්ඩය රැගෙන එන්න */}
      <div className="border-t border-dashed border-black/80 pt-2 text-center text-xs leading-snug font-medium text-black">
        මිලදී ගත් භාණ්ඩ මාරු කිරීම සදහා දින 7ක් ඇතුලත බිල්පත සමග භාණ්ඩය රැගෙන එන්න.
      </div>

      {/* 6. Thank you: ස්තූතියී, නැවත පැමිණෙන්න. */}
      <div className="text-center text-xs font-bold text-black">ස්තූතියී, නැවත පැමිණෙන්න!</div>

      {/* 7. Barcode of bill number */}
      <div className="my-1 flex justify-center py-1">
        <Barcode128 data={receipt.receipt_no} height={38} moduleWidth={1.5} />
      </div>

      {/* 8. Branding: Powered By BraceFrame - (076 9950 199) */}
      <div className="text-center text-[10px] text-neutral-800 font-mono">
        Powered By BraceFrame - (076 9950 199)
      </div>
    </div>
  );
}
