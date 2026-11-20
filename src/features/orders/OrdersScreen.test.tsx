import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import { commands, type OrderRow_Serialize as OrderRow } from "@/db/bindings";
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
  },
}));

function Exporter({ children }: { children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: ["REPORT_SALES", "DATA_EXPORT"] })), [setSession]);
  return <>{children}</>;
}

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

describe("OrdersScreen", () => {
  it("pages today's orders newest first and opens a receipt", async () => {
    renderWithProviders(<OrdersScreen />);
    const grid = await screen.findByRole("grid", { name: "Bills" });
    expect(await within(grid).findByText("T01-20261108-0003")).toBeInTheDocument();
    expect(within(grid).getByText("Voided · Scanned twice")).toBeInTheDocument();
    expect(within(grid).getByText("Completed · customer")).toBeInTheDocument();
    expect(commands.orderHistory).toHaveBeenCalledWith(
      { period: { kind: "day", date: "2026-11-08" }, device_id: null, status: null, receipt: null },
      null,
      200,
    );
    await waitFor(() =>
      expect(commands.orderHistory).toHaveBeenCalledWith(
        expect.anything(),
        { completed_at: "2026-11-08T09:02:00.000Z", id: "o2" },
        200,
      ),
    );
    expect(await within(grid).findByText("T01-20261108-0001")).toBeInTheDocument();
    fireEvent.click(within(grid).getByText("T01-20261108-0002"));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/07X XXX X567/)).toBeInTheDocument();
    expect(within(dialog).getByText("Rs 49.98")).toBeInTheDocument();
    expect(commands.orderReceipt).toHaveBeenCalledWith("o2");
  });

  it("filters by receipt, counter and status", async () => {
    renderWithProviders(<OrdersScreen />);
    await screen.findByRole("grid", { name: "Bills" });
    fireEvent.change(screen.getByLabelText("Receipt number"), { target: { value: "T01-2026" } });
    chooseOption(await screen.findByLabelText("Status"), "Voided");
    await waitFor(() =>
      expect(commands.orderHistory).toHaveBeenLastCalledWith(
        {
          period: { kind: "day", date: "2026-11-08" },
          device_id: null,
          status: "voided",
          receipt: "T01-2026",
        },
        null,
        200,
      ),
    );
  });

  it("exports the period's orders to a picked folder", async () => {
    vi.mocked(commands.exportPickFolder).mockImplementation(() => ok("E:\\exports"));
    vi.mocked(commands.exportCsv).mockImplementation(() =>
      ok({ path: "E:\\exports\\gayanshop-orders-2026-11-08-T01.csv", rows: 3 }),
    );
    renderWithProviders(
      <Exporter>
        <OrdersScreen />
      </Exporter>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Export orders" }));
    await waitFor(() =>
      expect(commands.exportCsv).toHaveBeenCalledWith(
        "orders",
        { kind: "day", date: "2026-11-08" },
        "E:\\exports",
      ),
    );
    expect(await screen.findByText(/Exported 3 rows to E:\\exports/)).toBeInTheDocument();
  });

  it("does nothing when the folder picker is cancelled", async () => {
    vi.mocked(commands.exportPickFolder).mockImplementation(() => ok(null));
    vi.mocked(commands.exportCsv).mockReset();
    renderWithProviders(
      <Exporter>
        <OrdersScreen />
      </Exporter>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Export order lines" }));
    await waitFor(() => expect(commands.exportPickFolder).toHaveBeenCalled());
    expect(commands.exportCsv).not.toHaveBeenCalled();
  });
});
