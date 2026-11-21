import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ReceiptData_Serialize as ReceiptData, ZReport_Serialize as ZReportData } from "@/db/bindings";
import { FM_LABELS } from "@/lib/unicodeToFm";
import { SinhalaReceiptTemplate } from "./SinhalaReceiptTemplate";
import { CashMovementTemplate, EstimateTemplate, ZReportTemplate } from "./templates";

const SAMPLE_RECEIPT: ReceiptData = {
  order_id: "order-123",
  receipt_no: "T01-20261120-0005",
  store_name: "Gayan Fashion",
  cashier: "Ruwan",
  customer_masked: "07X XXX X123",
  status: "completed",
  business_date: "2026-11-20",
  completed_at: "2026-11-20T18:25:00Z",
  item_count: 2,
  gross_sales: 240000,
  discount_total: 10000,
  grand_total: 230000,
  change_due: 20000,
  footer: "All sales final",
  lines: [
    {
      name: "Cotton Shirt Blue L",
      qty: 2,
      list_price: 120000,
      unit_price: 115000,
      line_gross: 240000,
      line_discount: 10000,
      line_total: 230000,
    },
  ],
  returns: [],
  exchange_credit: 0,
  payments: [
    {
      method: "cash",
      amount: 230000,
      tendered: 250000,
      reference: null,
    },
  ],
};

const SAMPLE_EXCHANGE_RECEIPT: ReceiptData = {
  ...SAMPLE_RECEIPT,
  receipt_no: "T01-20261120-0006",
  exchange_credit: 50000,
  returns: [
    {
      name: "Old T-Shirt",
      qty: 1,
      value: 50000,
      original_receipt_no: "T01-20261119-0001",
    },
  ],
};

const SAMPLE_ZREPORT: ZReportData = {
  session: {
    id: "session-1",
    device_id: "till-1",
    business_date: "2026-11-20",
    status: "closed",
    opened_at: "2026-11-20T08:00:00Z",
    closed_at: "2026-11-20T20:00:00Z",
    opened_by_name: "Ruwan",
    closed_by_name: "Ruwan",
    opening_float: 1000000,
    expected_cash: 6000000,
    counted_cash: 6000000,
    variance: 0,
    close_note: null,
    opened_by: "staff-1",
    closed_by: "staff-1",
  },
  generated_at: "2026-11-20T20:05:00Z",
  sales_count: 25,
  items_sold: 48,
  gross_sales: 5500000,
  discount_total: 500000,
  net_sales: 5000000,
  sales_total: 5000000,
  exchanges_count: 0,
  exchange_credit: 0,
  returned_items: 0,
  payments: [{ method: "cash", amount: 5000000, count: 25 }],
  change_given: 250000,
  overrides_count: 0,
  overrides_value: 0,
  quick_sale_lines: 0,
  quick_sale_net: 0,
  voids_count: 0,
  voids_value: 0,
  void_reasons: [],
  voids: [],
  drawer: [],
  pay_ins: 0,
  pay_outs: 0,
  no_sales: 0,
  opening_float: 1000000,
  cash_sales: 5000000,
  expected_cash: 6000000,
  counted_cash: 6000000,
  variance: 0,
};

describe("SinhalaReceiptTemplate (Sale Receipt)", () => {
  it("renders sale receipt with FM Font Sinhala labels", () => {
    render(<SinhalaReceiptTemplate receipt={SAMPLE_RECEIPT} />);

    // Header Details in FM Gemunu
    expect(screen.getByText(`${FM_LABELS.BILL_NO} :`)).toBeInTheDocument();
    expect(screen.getAllByText("T01-20261120-0005").length).toBe(2);
    expect(screen.getByText(`${FM_LABELS.DATE} :`)).toBeInTheDocument();
    expect(screen.getByText(`${FM_LABELS.TIME} :`)).toBeInTheDocument();
    expect(screen.getByText(`${FM_LABELS.CASHIER} :`)).toBeInTheDocument();
    expect(screen.getByText("Ruwan")).toBeInTheDocument();
    expect(screen.getByText("07X XXX X123")).toBeInTheDocument();

    // Items list
    expect(screen.getByText("Cotton Shirt Blue L")).toBeInTheDocument();
    expect(screen.getByText(/2 x 1,150.00/)).toBeInTheDocument();
    expect(screen.getAllByText("2,400.00").length).toBe(2);

    // Totals
    expect(screen.getByText(`${FM_LABELS.ITEMS_COUNT} :`)).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(screen.getByText(`${FM_LABELS.TOTAL} :`)).toBeInTheDocument();
    expect(screen.getByText("Rs 2,300.00")).toBeInTheDocument();
    expect(screen.getByText(`${FM_LABELS.PAID} :`)).toBeInTheDocument();
    expect(screen.getByText("Rs 2,500.00")).toBeInTheDocument();
    expect(screen.getByText(`${FM_LABELS.CHANGE} :`)).toBeInTheDocument();
    expect(screen.getByText("Rs 200.00")).toBeInTheDocument();

    // 7-day exchange policy & Thank you
    expect(screen.getByText(FM_LABELS.EXCHANGE_POLICY)).toBeInTheDocument();
    expect(screen.getByText(FM_LABELS.THANK_YOU)).toBeInTheDocument();

    // Barcode & Branding
    expect(screen.getByLabelText("Barcode: T01-20261120-0005")).toBeInTheDocument();
    expect(screen.getByText("Powered By BraceFrame - (076 9950 199)")).toBeInTheDocument();
  });

  it("omits text store name when a logo image is provided", () => {
    render(
      <SinhalaReceiptTemplate
        receipt={SAMPLE_RECEIPT}
        logoDataUrl="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
      />,
    );

    const img = screen.getByRole("img", { name: "Gayan Fashion" });
    expect(img).toBeInTheDocument();
    expect(screen.queryByRole("heading", { level: 1 })).not.toBeInTheDocument();
  });

  it("renders Return & Exchange template when return lines exist", () => {
    render(<SinhalaReceiptTemplate receipt={SAMPLE_EXCHANGE_RECEIPT} />);

    expect(screen.getByText(FM_LABELS.EXCHANGE_TITLE)).toBeInTheDocument();
    expect(screen.getByText(FM_LABELS.NEW_ITEMS)).toBeInTheDocument();
    expect(screen.getByText(FM_LABELS.RETURNED_ITEMS)).toBeInTheDocument();
    expect(screen.getByText("Old T-Shirt")).toBeInTheDocument();
  });
});

describe("Specialized Receipt Templates", () => {
  it("renders Z-Report template with accurate metrics and FM font labels", () => {
    render(<ZReportTemplate report={SAMPLE_ZREPORT} storeName="Gayan Fashion" />);

    expect(screen.getByText(FM_LABELS.Z_REPORT_TITLE)).toBeInTheDocument();
    expect(screen.getByText("2026-11-20")).toBeInTheDocument();
    expect(screen.getByText("25")).toBeInTheDocument();
    expect(screen.getByText("Rs 50,000.00")).toBeInTheDocument();
    expect(screen.getByText("Powered By BraceFrame - (076 9950 199)")).toBeInTheDocument();
  });

  it("renders Cash Movement template for Pay-In and Pay-Out", () => {
    render(
      <CashMovementTemplate
        data={{
          kind: "pay_in",
          amount: 500000,
          reason: "Morning float topup",
          staff: "Ruwan",
          created_at: "2026-11-20T08:15:00Z",
        }}
      />,
    );

    expect(screen.getByText(FM_LABELS.DRAWER_SLIP_TITLE)).toBeInTheDocument();
    expect(screen.getByText(FM_LABELS.PAY_IN_TITLE)).toBeInTheDocument();
    expect(screen.getByText("Rs 5,000.00")).toBeInTheDocument();
    expect(screen.getByText("Morning float topup")).toBeInTheDocument();
  });

  it("renders Estimate / Quotation template", () => {
    render(
      <EstimateTemplate
        data={{
          estimate_no: "EST-20261120-001",
          created_at: "2026-11-20T10:00:00Z",
          prepared_by: "Ruwan",
          customer_name: "Kamal Perera",
          lines: [
            {
              name: "Custom Curtains",
              qty: 4,
              unit_price: 350000,
              total: 1400000,
            },
          ],
          item_count: 4,
          total: 1400000,
        }}
      />,
    );

    expect(screen.getByText(FM_LABELS.ESTIMATE_TITLE)).toBeInTheDocument();
    expect(screen.getByText(FM_LABELS.NOT_A_SALE_NOTE)).toBeInTheDocument();
    expect(screen.getByText("EST-20261120-001")).toBeInTheDocument();
    expect(screen.getByText("Kamal Perera")).toBeInTheDocument();
    expect(screen.getByText("Rs 14,000.00")).toBeInTheDocument();
  });
});
