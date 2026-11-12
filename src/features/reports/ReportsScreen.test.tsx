import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { commands, type FinancialSummary_Serialize as FinancialSummary } from "@/db/bindings";
import { chooseOption, ok, renderWithProviders } from "@/test/render";
import { ReportsScreen, changeBp, formatBp, trendGranularity } from "./ReportsScreen";

vi.mock("@/db/bindings", () => ({
  commands: {
    businessToday: vi.fn(),
    reportDevices: vi.fn(),
    reportSummary: vi.fn(),
    reportSalesBy: vi.fn(),
    reportVoids: vi.fn(),
    reportTrend: vi.fn(),
    reportBatchSellThrough: vi.fn(),
  },
}));

function summary(over: Partial<FinancialSummary> = {}): FinancialSummary {
  return {
    range: { from: "2026-11-08", to: "2026-11-08" },
    device_id: null,
    sales_count: 40,
    void_count: 2,
    void_value: 9_000,
    gross_sales: 1_050_000,
    discounts: 50_000,
    net_sales: 1_000_000,
    tax_collected: 0,
    customer_orders: 10,
    customer_attached_bp: 2_500,
    avg_basket: 25_000,
    quick_sale_net: 40_000,
    unknown_cost_net: 40_000,
    payments: [
      { method: "cash", amount: 800_000, count: 35 },
      { method: "card", amount: 200_000, count: 5 },
    ],
    sales_cost: 600_000,
    allocation_variance: 0,
    cogs: 600_000,
    gross_profit: 400_000,
    gross_margin_bp: 4_000,
    stock_write_offs: 10_000,
    operating_expenses: 90_000,
    net_profit: 300_000,
    ...over,
  };
}

const NO_COSTS = {
  sales_cost: null,
  allocation_variance: null,
  cogs: null,
  gross_profit: null,
  gross_margin_bp: null,
  stock_write_offs: null,
  operating_expenses: null,
  net_profit: null,
};

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

beforeEach(() => {
  vi.mocked(commands.reportTrend).mockImplementation(() =>
    ok({
      current: [
        { bucket: "2026-11-02", sales_count: 3, net_sales: 90_000, sales_cost: 50_000, expenses: 10_000 },
        { bucket: "2026-11-03", sales_count: 0, net_sales: 0, sales_cost: 0, expenses: 0 },
      ],
      previous: [
        { bucket: "2026-10-26", sales_count: 1, net_sales: 30_000, sales_cost: 10_000, expenses: 0 },
        { bucket: "2026-10-27", sales_count: 0, net_sales: 0, sales_cost: 0, expenses: 0 },
      ],
    }),
  );
  vi.mocked(commands.reportBatchSellThrough).mockImplementation(() =>
    ok([
      {
        batch_id: "b1",
        code: "B-2026-1001",
        name: "Pettah",
        status: "received",
        received: 100,
        sold: 40,
        written_off: 2,
        on_hand: 58,
        sell_through_bp: 4_000,
      },
    ]),
  );
  vi.mocked(commands.businessToday).mockImplementation(() => ok("2026-11-08"));
  vi.mocked(commands.reportDevices).mockImplementation(() =>
    ok([
      { device_id: "d1", device_code: "T01" },
      { device_id: "d2", device_code: "T02" },
    ]),
  );
  vi.mocked(commands.reportSummary).mockImplementation(() =>
    ok({ current: summary(), previous: summary({ net_sales: 800_000, net_profit: 200_000 }) }),
  );
  vi.mocked(commands.reportSalesBy).mockImplementation(() =>
    ok([
      { id: "p1", label: "Bucket", qty: 12, net: 540_000, cost: 260_000 },
      { id: null, label: null, qty: 4, net: 40_000, cost: 0 },
    ]),
  );
  vi.mocked(commands.reportVoids).mockImplementation(() =>
    ok({
      count: 2,
      value: 9_000,
      by_reason: [{ key: "DUPLICATE_SCAN", label: null, count: 2, value: 9_000 }],
      by_approver: [{ key: "sup", label: "Saman", count: 2, value: 9_000 }],
    }),
  );
});

describe("ReportsScreen", () => {
  it("shows today's P&L against the previous day", async () => {
    renderWithProviders(<ReportsScreen />);
    const pnl = await screen.findByRole("table", { name: "Profit and loss" });
    expect(commands.reportSummary).toHaveBeenCalledWith({ kind: "day", date: "2026-11-08" }, null, true);
    const net = within(pnl).getByText("Net sales (revenue)").closest("tr");
    expect(net).not.toBeNull();
    if (net) {
      expect(within(net).getByText("Rs 10,000.00")).toBeInTheDocument();
      expect(within(net).getByText("Rs 8,000.00")).toBeInTheDocument();
      expect(within(net).getByText("25.0%")).toBeInTheDocument();
    }
    expect(within(pnl).getByText("Net profit")).toBeInTheDocument();
    const margin = within(pnl).getByText("Gross margin").closest("tr");
    expect(margin && within(margin).getAllByText("40.0%")).toHaveLength(2);
    expect(screen.getByText(/gross profit is higher than it really is/)).toBeInTheDocument();
  });

  it("shows payments, sales by product and voids", async () => {
    renderWithProviders(<ReportsScreen />);
    const payments = await screen.findByRole("region", { name: "Payments" });
    expect(within(payments).getByText("Card")).toBeInTheDocument();
    const salesBy = screen.getByRole("region", { name: "Sales by" });
    expect(await within(salesBy).findByText("Bucket")).toBeInTheDocument();
    expect(within(salesBy).getByText("Quick sales")).toBeInTheDocument();
    expect(within(salesBy).getByText("Rs 2,800.00")).toBeInTheDocument();
    fireEvent.click(within(salesBy).getByRole("button", { name: "Category" }));
    await waitFor(() =>
      expect(commands.reportSalesBy).toHaveBeenLastCalledWith(
        { kind: "day", date: "2026-11-08" },
        null,
        "category",
      ),
    );
    const voids = await screen.findByRole("region", { name: "Voids" });
    expect(within(voids).getByText("Scanned twice")).toBeInTheDocument();
    expect(within(voids).getByText("Saman")).toBeInTheDocument();
  });

  it("filters by counter and period", async () => {
    renderWithProviders(<ReportsScreen />);
    await screen.findByRole("table", { name: "Profit and loss" });
    chooseOption(await screen.findByLabelText("Counter"), "T02");
    await waitFor(() =>
      expect(commands.reportSummary).toHaveBeenLastCalledWith(
        { kind: "day", date: "2026-11-08" },
        "d2",
        true,
      ),
    );
    expect(screen.getByText(/stay store-wide/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "All time" }));
    await waitFor(() =>
      expect(commands.reportSummary).toHaveBeenLastCalledWith({ kind: "all_time" }, "d2", false),
    );
  });

  it("hides cost lines when Rust leaves them out", async () => {
    vi.mocked(commands.reportSummary).mockImplementation(() =>
      ok({ current: summary(NO_COSTS), previous: null }),
    );
    vi.mocked(commands.reportSalesBy).mockImplementation(() =>
      ok([{ id: "p1", label: "Bucket", qty: 12, net: 540_000, cost: null }]),
    );
    renderWithProviders(<ReportsScreen />);
    const pnl = await screen.findByRole("table", { name: "Profit and loss" });
    expect(within(pnl).queryByText("Net profit")).not.toBeInTheDocument();
    expect(within(pnl).queryByText("Previous")).not.toBeInTheDocument();
    expect(screen.getByText(/need the Financial reports/)).toBeInTheDocument();
    const salesBy = screen.getByRole("region", { name: "Sales by" });
    await within(salesBy).findByText("Bucket");
    expect(within(salesBy).queryByText("Gross profit")).not.toBeInTheDocument();
  });

  it("charts a week's trend against the week before, with payments and sell-through", async () => {
    renderWithProviders(<ReportsScreen />);
    await screen.findByRole("table", { name: "Profit and loss" });
    expect(screen.queryByRole("figure", { name: "Sales trend" })).not.toBeInTheDocument();
    expect(await screen.findByRole("figure", { name: "Payment methods" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Week" }));
    const trend = await screen.findByRole("figure", { name: "Sales trend" });
    expect(within(trend).getByText("Rs 300.00")).toBeInTheDocument();
    expect(screen.getByRole("figure", { name: "Revenue and expenses" })).toBeInTheDocument();
    expect(commands.reportTrend).toHaveBeenLastCalledWith(
      { kind: "week", any_date: "2026-11-08" },
      null,
      "day",
      true,
    );
  });

  it("buckets the trend by the period's length", () => {
    expect(trendGranularity({ kind: "day", date: "2026-11-08" })).toBeNull();
    expect(trendGranularity({ kind: "month", year: 2026, month: 11 })).toBe("day");
    expect(trendGranularity({ kind: "year", year: 2026 })).toBe("month");
    expect(trendGranularity({ kind: "custom", from: "2026-01-01", to: "2026-02-15" })).toBe("day");
    expect(trendGranularity({ kind: "custom", from: "2026-01-01", to: "2026-09-30" })).toBe("week");
    expect(trendGranularity({ kind: "custom", from: "2024-01-01", to: "2026-09-30" })).toBe("month");
    expect(trendGranularity({ kind: "all_time" })).toBe("month");
  });

  it("formats changes and margins", () => {
    expect(formatBp(2_550)).toBe("25.5%");
    expect(formatBp(-1_234)).toBe("-12.3%");
    expect(changeBp(1_000_000, 800_000)).toBe(2_500);
    expect(changeBp(600, 800)).toBe(-2_500);
    expect(changeBp(5, 0)).toBeNull();
    expect(changeBp(-50, -100)).toBe(5_000);
  });
});
