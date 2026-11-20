import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { SinhalaReceiptTemplate } from "./SinhalaReceiptTemplate";

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
  payments: [
    {
      method: "cash",
      amount: 230000,
      tendered: 250000,
      reference: null,
    },
  ],
};

describe("SinhalaReceiptTemplate", () => {
  it("renders all receipt sections in Sinhala layout requested by user", () => {
    render(<SinhalaReceiptTemplate receipt={SAMPLE_RECEIPT} />);

    // Header Details
    expect(screen.getByText("බිල් අංකය :")).toBeInTheDocument();
    expect(screen.getAllByText("T01-20261120-0005").length).toBe(2);
    expect(screen.getByText("දිනය :")).toBeInTheDocument();
    expect(screen.getByText("වේලාව :")).toBeInTheDocument();
    expect(screen.getByText("අයකැමි :")).toBeInTheDocument();
    expect(screen.getByText("Ruwan")).toBeInTheDocument();
    expect(screen.getByText("07X XXX X123")).toBeInTheDocument();

    // Items list
    expect(screen.getByText("Cotton Shirt Blue L")).toBeInTheDocument();
    expect(screen.getByText(/2 x 1,150.00/)).toBeInTheDocument();
    expect(screen.getByText("2,400.00")).toBeInTheDocument();

    // Totals
    expect(screen.getByText("භාණ්ඩ ගණන :")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(screen.getByText("මුළු මුදල :")).toBeInTheDocument();
    expect(screen.getByText("Rs 2,300.00")).toBeInTheDocument();
    expect(screen.getByText("ගෙවූ මුදල :")).toBeInTheDocument();
    expect(screen.getByText("Rs 2,500.00")).toBeInTheDocument();
    expect(screen.getByText("ඉතිරි මුදල :")).toBeInTheDocument();
    expect(screen.getByText("Rs 200.00")).toBeInTheDocument();

    // 7-day exchange policy
    expect(
      screen.getByText("මිලදී ගත් භාණ්ඩ මාරු කිරීම සදහා දින 7ක් ඇතුලත බිල්පත සමග භාණ්ඩය රැගෙන එන්න."),
    ).toBeInTheDocument();

    // Thank you note
    expect(screen.getByText("ස්තූතියී, නැවත පැමිණෙන්න!")).toBeInTheDocument();

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
});
