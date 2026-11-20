import { act, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, type ReceiptData_Serialize as ReceiptData, type ReceiptImage } from "@/db/bindings";
import { receiptImageFromElement } from "@/lib/receiptImage";
import { err, ok, renderWithProviders } from "@/test/render";
import { usePrintReceiptStore } from "./PrintableReceiptPortal";

vi.mock("@/db/bindings", () => ({ commands: { printReceiptImage: vi.fn() } }));
vi.mock("@/lib/receiptImage", () => ({ receiptImageFromElement: vi.fn() }));

const RECEIPT: ReceiptData = {
  order_id: "order-1",
  receipt_no: "T01-20261121-0001",
  store_name: "Gayan Fashion",
  cashier: "Ruwan",
  customer_masked: null,
  status: "completed",
  business_date: "2026-11-21",
  completed_at: "2026-11-21T10:00:00Z",
  item_count: 1,
  gross_sales: 45000,
  discount_total: 0,
  grand_total: 45000,
  change_due: 0,
  footer: "",
  lines: [
    {
      name: "Cotton Shirt",
      qty: 1,
      list_price: 45000,
      unit_price: 45000,
      line_gross: 45000,
      line_discount: 0,
      line_total: 45000,
    },
  ],
  returns: [],
  exchange_credit: 0,
  payments: [{ method: "cash", amount: 45000, tendered: null, reference: null }],
};

const IMAGE: ReceiptImage = { width: 8, height: 2, data: [0xff, 0x00] };

beforeEach(() => {
  // jsdom has no font loading; the webview does.
  Object.defineProperty(document, "fonts", { configurable: true, value: { ready: Promise.resolve() } });
  usePrintReceiptStore.getState().clear();
  vi.mocked(receiptImageFromElement).mockResolvedValue(IMAGE);
  vi.mocked(commands.printReceiptImage).mockImplementation(() => ok(null));
  vi.spyOn(window, "print").mockImplementation(() => undefined);
});

describe("PrintableReceiptPortal", () => {
  it("draws the receipt, queues it for the printer and never opens the print dialog", async () => {
    renderWithProviders(<div />);
    act(() => usePrintReceiptStore.getState().print(RECEIPT));

    await waitFor(() => expect(commands.printReceiptImage).toHaveBeenCalledWith(IMAGE));
    expect(await screen.findByText("Receipt sent to the printer.")).toBeInTheDocument();
    await waitFor(() => expect(usePrintReceiptStore.getState().activeReceipt).toBeNull());
    expect(window.print).not.toHaveBeenCalled();
  });

  it("shows the printer's refusal and still lets go of the receipt", async () => {
    vi.mocked(commands.printReceiptImage).mockImplementation(() =>
      err({ kind: "FORBIDDEN", perm: "REPRINT_RECEIPT" }),
    );
    renderWithProviders(<div />);
    act(() => usePrintReceiptStore.getState().print(RECEIPT));

    expect(await screen.findByText("You don't have permission to do this.")).toBeInTheDocument();
    expect(commands.printReceiptImage).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(usePrintReceiptStore.getState().activeReceipt).toBeNull());
  });

  it("says so when the receipt can't be drawn, and sends nothing", async () => {
    vi.mocked(receiptImageFromElement).mockRejectedValue(new Error("no width"));
    renderWithProviders(<div />);
    act(() => usePrintReceiptStore.getState().print(RECEIPT));

    expect(await screen.findByText("The receipt couldn't be prepared for printing.")).toBeInTheDocument();
    expect(commands.printReceiptImage).not.toHaveBeenCalled();
    await waitFor(() => expect(usePrintReceiptStore.getState().activeReceipt).toBeNull());
  });
});
