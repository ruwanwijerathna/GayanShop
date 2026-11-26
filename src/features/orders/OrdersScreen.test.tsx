import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import { commands, type OrderRow_Serialize as OrderRow, type Perm } from "@/db/bindings";
import { chooseOption, ok, renderWithProviders, session } from "@/test/render";
import { OrdersScreen } from "./OrdersScreen";

vi.mock("@/db/bindings", () => ({
  commands: {
    businessToday: vi.fn(),
    reportDevices: vi.fn(),
    orderHistory: vi.fn(),
    orderReceipt: vi.fn(),
    exportPickFolder: vi.fn(),
    exportCsv: vi.fn(),
    printReceiptImage: vi.fn(),
  },
}));
vi.mock("@/lib/receiptImage", () => ({
  receiptImageFromElement: vi.fn(() => Promise.resolve({ width: 8, height: 1, data: [255] })),
}));

function As({ perms, children }: { perms: Perm[]; children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: perms })), [setSession, perms]);
  return <>{children}</>;
}

const MANAGER: Perm[] = ["REPORT_SALES", "REPRINT_RECEIPT", "DATA_EXPORT"];
const CASHIER: Perm[] = ["REPRINT_RECEIPT"];

function renderAs(perms: Perm[], scanned: { receipt: string; at: number } | null = null) {
  return renderWithProviders(
    <As perms={perms}>
      <OrdersScreen scanned={scanned} />
    </As>,
  );
}

const FILTER = {
  period: { kind: "all_time" },
  device_id: null,
  status: null,
  receipt: null,
  phone: null,
  payment: null,
  kind: null,
};

function order(n: number, status = "completed"): OrderRow {
  return {
    id: `o${n}`,
    receipt_no: `T01-20261108-${String(n).padStart(4, "0")}`,
    business_date: "2026-11-08",
    completed_at: `2026-11-08T09:${String(n).padStart(2, "0")}:00.000Z`,
    status,
    cashier: "Nimal",
    item_count: 2,
    grand_total: 45_000 + n,
    has_customer: n === 2,
    void_reason: status === "voided" ? "DUPLICATE_SCAN" : null,
    exchange_credit: n === 2 ? 45_000 : 0,
    has_returns: n === 1,
    methods: n === 3 ? "card,cash" : "cash",
  };
}

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 520 });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 1100 });
});

beforeEach(() => {
  vi.mocked(commands.businessToday).mockImplementation(() => ok("2026-11-08"));
  vi.mocked(commands.reportDevices).mockImplementation(() => ok([{ device_id: "d1", device_code: "T01" }]));
  vi.mocked(commands.orderHistory).mockReset();
  vi.mocked(commands.orderHistory)
    .mockImplementationOnce(() =>
      ok({
        rows: [order(3, "voided"), order(2)],
        next_cursor: { completed_at: "2026-11-08T09:02:00.000Z", id: "o2" },
      }),
    )
    .mockImplementation(() => ok({ rows: [order(1)], next_cursor: null }));
  vi.mocked(commands.printReceiptImage).mockImplementation(() => ok(null));
  Object.defineProperty(document, "fonts", { configurable: true, value: { ready: Promise.resolve() } });
  vi.mocked(commands.orderReceipt).mockImplementation(() =>
    ok({
      order_id: "o2",
      store_name: "GayanShop",
      receipt_no: "T01-20261108-0002",
      status: "completed",
      business_date: "2026-11-08",
      completed_at: "2026-11-08T09:02:00.000Z",
      cashier: "Nimal",
      lines: [
        {
          name: "Bucket",
          qty: 1,
          list_price: 45_002,
          unit_price: 45_002,
          line_gross: 45_002,
          line_discount: 0,
          line_total: 45_002,
        },
      ],
      item_count: 1,
      gross_sales: 45_002,
      discount_total: 0,
      grand_total: 45_002,
      payments: [{ method: "cash", amount: 45_002, tendered: 50_000, reference: null }],
      change_due: 4_998,
      returns: [],
      exchange_credit: 0,
      customer_masked: "07X XXX X567",
      footer: "Exchange within 7 days. Item for item only; no cash refunds. Thank you, come again!",
    }),
  );
});

describe("Bills", () => {
  it("lists every bill by default, newest first, with how each was paid and what kind it is, and opens a receipt", async () => {
    renderAs(MANAGER);
    const grid = await screen.findByRole("grid", { name: "Bills" });
    expect(await within(grid).findByText("T01-20261108-0003")).toBeInTheDocument();
    expect(commands.orderHistory).toHaveBeenCalledWith(FILTER, null, 200);
    expect(screen.getByLabelText("Period")).toHaveTextContent("All");
    const voided = within(grid).getByText("T01-20261108-0003").closest("tr");
    if (!voided) throw new Error("no row");
    expect(within(voided).getByText("Card")).toBeInTheDocument();
    expect(within(voided).getByText("Cash")).toBeInTheDocument();
    expect(within(voided).getByText("Voided")).toBeInTheDocument();
    expect(within(voided).getByText("Scanned twice")).toBeInTheDocument();
    expect(within(grid).getByText("Exchange")).toBeInTheDocument();

    // Pagination footer matches Products & Stock, and more bills load when reaching the last loaded page.
    await waitFor(() =>
      expect(commands.orderHistory).toHaveBeenCalledWith(
        expect.anything(),
        { completed_at: "2026-11-08T09:02:00.000Z", id: "o2" },
        200,
      ),
    );
    expect(await within(grid).findByText("T01-20261108-0001")).toBeInTheDocument();
    expect(within(grid).getByText("Has returns")).toBeInTheDocument();
    expect(screen.getByText("0 of 3 selected")).toBeInTheDocument();
    expect(screen.getByText("Page 1 of 1")).toBeInTheDocument();
    fireEvent.click(within(grid).getByText("T01-20261108-0002"));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/07X XXX X567/)).toBeInTheDocument();
    expect(commands.orderReceipt).toHaveBeenCalledWith("o2");
  });

  it("shows each bill's time, cashier, items, payment, status and total, and hides columns from View", async () => {
    renderAs(MANAGER);
    const grid = await screen.findByRole("grid", { name: "Bills" });
    const row = (await within(grid).findByText("T01-20261108-0002")).closest("tr");
    if (!row) throw new Error("no row");
    for (const header of ["Receipt no.", "Date & time", "Cashier", "Items", "Payment", "Status", "Total"]) {
      expect(within(grid).getByRole("columnheader", { name: header })).toBeInTheDocument();
    }
    expect(within(row).getByText("Nimal")).toBeInTheDocument();
    expect(within(row).getByText("2")).toBeInTheDocument();
    expect(within(row).getByText("Completed")).toBeInTheDocument();
    expect(within(row).getByText("Exchange credit Rs 450.00")).toBeInTheDocument();
    expect(within(row).getByText("Rs 450.02")).toBeInTheDocument();
    const voidedTotal = within(grid).getByText("Rs 450.03");
    expect(voidedTotal).toHaveClass("line-through");

    fireEvent.pointerDown(screen.getByRole("button", { name: "View" }), { button: 0, ctrlKey: false });
    fireEvent.click(await screen.findByRole("menuitemcheckbox", { name: "Cashier" }));
    await waitFor(() =>
      expect(within(grid).queryByRole("columnheader", { name: "Cashier" })).not.toBeInTheDocument(),
    );
    expect(within(grid).queryByText("Nimal")).not.toBeInTheDocument();
  });

  it("sorts from the column menu", async () => {
    renderAs(MANAGER);
    const grid = await screen.findByRole("grid", { name: "Bills" });
    await within(grid).findByText("T01-20261108-0001");
    fireEvent.pointerDown(within(grid).getByRole("button", { name: /Receipt no\./ }), { button: 0, ctrlKey: false });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Asc" }));
    const receipts = within(grid)
      .getAllByRole("row")
      .slice(1)
      .map((r) => within(r).getByText(/^T01-/).textContent);
    expect(receipts).toEqual(["T01-20261108-0001", "T01-20261108-0002", "T01-20261108-0003"]);
  });

  it("opens a bill from the keyboard or its row button", async () => {
    renderAs(MANAGER);
    const grid = await screen.findByRole("grid", { name: "Bills" });
    const row = (await within(grid).findByText("T01-20261108-0002")).closest("tr");
    if (!row) throw new Error("no row");
    fireEvent.keyDown(row, { key: "Enter" });
    await waitFor(() => expect(commands.orderReceipt).toHaveBeenCalledWith("o2"));
    const [close] = within(await screen.findByRole("dialog")).getAllByRole("button", { name: /Close/ });
    if (!close) throw new Error("no close button");
    fireEvent.click(close);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    fireEvent.click(within(grid).getByRole("button", { name: "View or print bill T01-20261108-0003" }));
    await waitFor(() => expect(commands.orderReceipt).toHaveBeenCalledWith("o3"));
  });

  it("jumps between periods from the toolbar dropdown", async () => {
    renderAs(MANAGER);
    await screen.findByRole("grid", { name: "Bills" });
    chooseOption(screen.getByLabelText("Period"), "Today");
    await waitFor(() =>
      expect(commands.orderHistory).toHaveBeenLastCalledWith(
        { ...FILTER, period: { kind: "day", date: "2026-11-08" } },
        null,
        200,
      ),
    );
    chooseOption(screen.getByLabelText("Period"), "Yesterday");
    await waitFor(() =>
      expect(commands.orderHistory).toHaveBeenLastCalledWith(
        { ...FILTER, period: { kind: "day", date: "2026-11-07" } },
        null,
        200,
      ),
    );
    chooseOption(screen.getByLabelText("Period"), "This week");
    await waitFor(() =>
      expect(commands.orderHistory).toHaveBeenLastCalledWith(
        { ...FILTER, period: { kind: "week", any_date: "2026-11-08" } },
        null,
        200,
      ),
    );
    chooseOption(screen.getByLabelText("Period"), "Pick a date");
    fireEvent.change(screen.getByLabelText("Pick a date", { selector: "input" }), {
      target: { value: "2026-10-30" },
    });
    await waitFor(() =>
      expect(commands.orderHistory).toHaveBeenLastCalledWith(
        { ...FILTER, period: { kind: "day", date: "2026-10-30" } },
        null,
        200,
      ),
    );
  });

  it("finds a bill by receipt number or phone across all dates", async () => {
    renderAs(MANAGER);
    await screen.findByRole("grid", { name: "Bills" });
    const box = screen.getByLabelText("Search bills");

    fireEvent.change(box, { target: { value: "T01-2026" } });
    await waitFor(() =>
      expect(commands.orderHistory).toHaveBeenLastCalledWith(
        { ...FILTER, period: { kind: "all_time" }, receipt: "T01-2026" },
        null,
        200,
      ),
    );
    expect(screen.getByText("Searching all dates")).toBeInTheDocument();
    expect(screen.queryByLabelText("Period")).not.toBeInTheDocument();

    fireEvent.change(box, { target: { value: "261126001" } });
    await waitFor(() =>
      expect(commands.orderHistory).toHaveBeenLastCalledWith(
        { ...FILTER, period: { kind: "all_time" }, receipt: "261126001", phone: "261126001" },
        null,
        200,
      ),
    );

    fireEvent.change(box, { target: { value: "077 123 4567" } });
    await waitFor(() =>
      expect(commands.orderHistory).toHaveBeenLastCalledWith(
        { ...FILTER, period: { kind: "all_time" }, phone: "077 123 4567" },
        null,
        200,
      ),
    );

    fireEvent.change(box, { target: { value: "" } });
    expect(await screen.findByLabelText("Period")).toBeInTheDocument();
  });

  it("waits for four digits before searching a phone", async () => {
    renderAs(MANAGER);
    await screen.findByRole("grid", { name: "Bills" });
    fireEvent.change(screen.getByLabelText("Search bills"), { target: { value: "077" } });
    expect(await screen.findByText("Type at least 4 digits of the phone number.")).toBeInTheDocument();
    await new Promise((r) => setTimeout(r, 350));
    expect(commands.orderHistory).not.toHaveBeenCalledWith(
      expect.objectContaining({ phone: expect.anything() }),
      expect.anything(),
      expect.anything(),
    );
  });

  it("narrows by payment and period in the toolbar, and counter, type and status in the Filters dialog", async () => {
    renderAs(MANAGER);
    await screen.findByRole("grid", { name: "Bills" });
    expect(screen.queryByRole("button", { name: /Reset/ })).not.toBeInTheDocument();
    chooseOption(screen.getByLabelText("Paid with"), "Card");

    fireEvent.click(screen.getByRole("button", { name: "Filters" }));
    const dialog = await screen.findByRole("dialog", { name: "More filters" });
    chooseOption(within(dialog).getByLabelText("Type"), "Exchange bills");
    chooseOption(within(dialog).getByLabelText("Counter"), "T01");
    chooseOption(within(dialog).getByLabelText("Status"), "Voided only");
    fireEvent.click(within(dialog).getByRole("button", { name: "Done" }));

    await waitFor(() =>
      expect(commands.orderHistory).toHaveBeenLastCalledWith(
        { ...FILTER, device_id: "d1", status: "voided", payment: "card", kind: "exchange" },
        null,
        200,
      ),
    );
    expect(screen.getByRole("button", { name: "Filters (3)" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Reset/ }));
    // Back to the first query: its page is still cached, so the screen simply shows it again.
    await waitFor(() => expect(screen.queryByRole("button", { name: /Reset/ })).not.toBeInTheDocument());
    expect(screen.getByLabelText("Paid with")).toHaveTextContent("Any payment");
  });

  it("shows a cashier this counter's bills without a counter choice", async () => {
    renderAs(CASHIER);
    await screen.findByRole("grid", { name: "Bills" });
    expect(screen.getByLabelText("Paid with")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Filters" }));
    const dialog = await screen.findByRole("dialog", { name: "More filters" });
    expect(within(dialog).queryByLabelText("Counter")).not.toBeInTheDocument();
    expect(commands.reportDevices).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Export" })).not.toBeInTheDocument();
  });

  it("prints a bill again straight to the printer and records it against that bill", async () => {
    renderAs(CASHIER);
    const grid = await screen.findByRole("grid", { name: "Bills" });
    fireEvent.click(await within(grid).findByText("T01-20261108-0002"));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(await within(dialog).findByRole("button", { name: /Print/ }));
    await waitFor(() =>
      expect(commands.printReceiptImage).toHaveBeenCalledWith({ width: 8, height: 1, data: [255] }, "o2"),
    );
  });

  it("exports the bills of the chosen period to a picked folder", async () => {
    vi.mocked(commands.exportPickFolder).mockImplementation(() => ok("E:\\exports"));
    vi.mocked(commands.exportCsv).mockImplementation(() =>
      ok({ path: "E:\\exports\\gayanshop-orders-2026-11-08-T01.csv", rows: 3 }),
    );
    renderAs(MANAGER);
    fireEvent.keyDown(await screen.findByRole("button", { name: "Export" }), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Export bills" }));
    await waitFor(() =>
      expect(commands.exportCsv).toHaveBeenCalledWith("orders", { kind: "all_time" }, "E:\\exports"),
    );
    expect(await screen.findByText(/Exported 3 rows to E:\\exports/)).toBeInTheDocument();
  });

  it("does nothing when the folder picker is cancelled", async () => {
    vi.mocked(commands.exportPickFolder).mockImplementation(() => ok(null));
    vi.mocked(commands.exportCsv).mockReset();
    renderAs(MANAGER);
    fireEvent.keyDown(await screen.findByRole("button", { name: "Export" }), { key: "Enter" });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Export bill items" }));
    await waitFor(() => expect(commands.exportPickFolder).toHaveBeenCalled());
    expect(commands.exportCsv).not.toHaveBeenCalled();
  });

  it("finds a bill scanned elsewhere in the app across all dates and opens its receipt", async () => {
    vi.mocked(commands.orderHistory).mockReset();
    vi.mocked(commands.orderHistory).mockImplementation(() => ok({ rows: [order(2)], next_cursor: null }));
    renderAs(MANAGER, { receipt: "T01-20261108-0002", at: 1 });
    expect(await screen.findByRole("searchbox", { name: "Search bills" })).toHaveValue("T01-20261108-0002");
    await waitFor(() => expect(commands.orderReceipt).toHaveBeenCalledWith("o2"));
    expect(commands.orderHistory).toHaveBeenCalledWith(
      expect.objectContaining({ period: { kind: "all_time" }, receipt: "T01-20261108-0002" }),
      null,
      200,
    );
  });
});
