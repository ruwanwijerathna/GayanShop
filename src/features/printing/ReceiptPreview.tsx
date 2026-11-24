// Settings → Receipt printer: a live preview of the receipt template
// (header image, shop name, sample lines, totals, QR/barcode, and footer),
// using the universal SinhalaReceiptTemplate so any template changes are immediately visible here.
import { useTranslation } from "react-i18next";
import type { PaperWidth, ReceiptData_Serialize as ReceiptData, ReceiptHeaderView } from "@/db/bindings";
import { SinhalaReceiptTemplate } from "@/components/receipt/SinhalaReceiptTemplate";

export const SAMPLE_RECEIPT: ReceiptData = {
  order_id: "SAMPLE-RECEIPT",
  store_name: "Sales Center",
  receipt_no: "T01-261120001",
  status: "completed",
  business_date: "2026-11-20",
  completed_at: "2026-11-20T20:30:00",
  cashier: "Cashier 01",
  customer_masked: null,
  lines: [
    {
      name: "Sample item A",
      qty: 2,
      list_price: 45000,
      unit_price: 45000,
      line_gross: 90000,
      line_discount: 0,
      line_total: 90000,
    },
    {
      name: "Sample item B",
      qty: 1,
      list_price: 125000,
      unit_price: 125000,
      line_gross: 125000,
      line_discount: 0,
      line_total: 125000,
    },
    {
      name: "Sample item C",
      qty: 3,
      list_price: 9500,
      unit_price: 9500,
      line_gross: 28500,
      line_discount: 0,
      line_total: 28500,
    },
  ],
  item_count: 6,
  gross_sales: 243500,
  discount_total: 0,
  grand_total: 243500,
  payments: [{ method: "cash", amount: 243500, tendered: 300000, reference: null }],
  change_due: 56500,
  returns: [],
  exchange_credit: 0,
  footer: "",
};

export function ReceiptPreview({
  header,
  width,
  qr = false,
}: {
  header: ReceiptHeaderView | undefined;
  width: PaperWidth;
  qr?: boolean;
}) {
  const { t } = useTranslation("printing");

  const sample: ReceiptData = {
    ...SAMPLE_RECEIPT,
    store_name: header?.store_name || SAMPLE_RECEIPT.store_name,
    footer: "",
  };

  return (
    <figure className="flex flex-col items-center gap-2">
      <div
        role="img"
        aria-label={t("preview.title")}
        className="w-full max-w-[72mm] rounded-sm border border-border bg-white shadow-sm overflow-hidden"
      >
        <SinhalaReceiptTemplate
          receipt={sample}
          logo={header?.logo}
          storeName={header?.store_name}
          qr={qr}
          paperWidth={width}
        />
      </div>
      <figcaption className="text-sm text-muted-foreground">{t("preview.title")}</figcaption>
    </figure>
  );
}
