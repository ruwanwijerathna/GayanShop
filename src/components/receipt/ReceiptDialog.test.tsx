import { fireEvent, screen } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import type { ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { renderWithProviders } from "@/test/render";
import { ReceiptDialog } from "./ReceiptDialog";
import { usePrintReceiptStore } from "./PrintableReceiptPortal";

const SAMPLE_RECEIPT: ReceiptData = {
  order_id: "order-123",
  receipt_no: "T01-20261124-0005",
  store_name: "Gayan Sales Center",
  cashier: "Cashier 01",
  customer_masked: null,
  status: "completed",
  business_date: "2026-11-24",
  completed_at: "2026-11-24T14:30:00Z",
  item_count: 2,
  gross_sales: 150000,
  discount_total: 0,
  grand_total: 150000,
  change_due: 50000,
  footer: "Thank you, come again!",
  lines: [
    {
      name: "Men Cotton Shirt",
      qty: 1,
      list_price: 150000,
      unit_price: 150000,
      line_gross: 150000,
      line_discount: 0,
      line_total: 150000,
    },
  ],
  returns: [],
  exchange_credit: 0,
  payments: [{ method: "cash", amount: 150000, tendered: 200000, reference: null }],
};

describe("ReceiptDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders receipt preview dialog with max-height constraint and bill details", () => {
    const onClose = vi.fn();
    renderWithProviders(<ReceiptDialog receipt={SAMPLE_RECEIPT} onClose={onClose} />);

    // Dialog content should have max-height constraint leaving screen margin
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveClass("max-h-[calc(100vh-3.5rem)]");
    expect(dialog).not.toHaveClass("h-screen");

    // Bill details
    expect(screen.getByText("#T01-20261124-0005")).toBeInTheDocument();
    expect(screen.getByText("Men Cotton Shirt")).toBeInTheDocument();
    expect(screen.getByText("Cashier 01")).toBeInTheDocument();
  });

  it("triggers print and closes on Print button click", () => {
    const onClose = vi.fn();
    const spy = vi.spyOn(usePrintReceiptStore.getState(), "print").mockImplementation(() => undefined);

    renderWithProviders(<ReceiptDialog receipt={SAMPLE_RECEIPT} onClose={onClose} isReprint={false} />);

    const printBtn = screen.getByRole("button", { name: /Print/i });
    fireEvent.click(printBtn);

    expect(spy).toHaveBeenCalledWith(SAMPLE_RECEIPT, { reprint: false });
    expect(onClose).toHaveBeenCalled();
  });

  it("triggers print on Enter key press", () => {
    const onClose = vi.fn();
    const spy = vi.spyOn(usePrintReceiptStore.getState(), "print").mockImplementation(() => undefined);

    renderWithProviders(<ReceiptDialog receipt={SAMPLE_RECEIPT} onClose={onClose} isReprint={false} />);

    fireEvent.keyDown(window, { key: "Enter" });

    expect(spy).toHaveBeenCalledWith(SAMPLE_RECEIPT, { reprint: false });
    expect(onClose).toHaveBeenCalled();
  });

  it("calls onClose on Close button click", () => {
    const onClose = vi.fn();
    renderWithProviders(<ReceiptDialog receipt={SAMPLE_RECEIPT} onClose={onClose} />);

    const closeBtns = screen.getAllByRole("button", { name: /Close/i });
    const closeBtn = closeBtns[0];
    expect(closeBtn).toBeDefined();
    if (closeBtn) fireEvent.click(closeBtn);

    expect(onClose).toHaveBeenCalled();
  });
});
