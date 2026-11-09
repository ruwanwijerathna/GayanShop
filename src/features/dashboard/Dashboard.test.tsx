import { screen, waitFor, within } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { commands, type FinancialSummary_Serialize as FinancialSummary } from "@/db/bindings";
import { ok, renderWithProviders } from "@/test/render";
import { Dashboard, addDays } from "./Dashboard";

vi.mock("@/db/bindings", () => ({
  commands: {
    businessToday: vi.fn(),
    reportSummary: vi.fn(),
    reportTrend: vi.fn(),
    customerAnalytics: vi.fn(),
  },
}));

function summary(net: number, profit: number | null): FinancialSummary {
  return {
    range: { from: "2026-11-08", to: "2026-11-08" },
    device_id: null,
    sales_count: 20,
    void_count: 0,
    void_value: 0,
    gross_sales: net,
    discounts: 0,
    net_sales: net,
    tax_collected: 0,
    customer_orders: 5,
    customer_attached_bp: 2_500,
    avg_basket: net / 20,
    quick_sale_net: 0,
    unknown_cost_net: 0,
    payments: [{ method: "cash", amount: net, count: 20 }],
    sales_cost: profit === null ? null : net - profit,
    allocation_variance: profit === null ? null : 0,
    cogs: profit === null ? null : net - profit,
    gross_profit: profit,
    gross_margin_bp: profit === null ? null : 4_000,
    stock_write_offs: profit === null ? null : 0,
    operating_expenses: profit === null ? null : 0,
    net_profit: profit,
  };
}

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

beforeEach(() => {
  vi.mocked(commands.businessToday).mockImplementation(() => ok("2026-11-08"));
  vi.mocked(commands.reportSummary).mockImplementation(() =>
    ok({ current: summary(1_000_000, 400_000), previous: summary(800_000, 500_000) }),
  );
  vi.mocked(commands.reportTrend).mockImplementation(() =>
    ok({
      current: [{ bucket: "2026-11-08", sales_count: 20, net_sales: 1_000_000, sales_cost: 0, expenses: 0 }],
      previous: null,
    }),
  );
});

describe("Dashboard", () => {
  it("shows today against yesterday with the last 30 days and this year", async () => {
    renderWithProviders(<Dashboard />);
    const board = await screen.findByRole("region", { name: "Today" });
    expect(await within(board).findByText("Rs 10,000.00", { selector: "span" })).toBeInTheDocument();
    expect(within(board).getAllByText("+25.0% vs yesterday")).toHaveLength(2);
    expect(within(board).getByText("-20.0% vs yesterday")).toBeInTheDocument();
    expect(commands.reportSummary).toHaveBeenCalledWith({ kind: "day", date: "2026-11-08" }, null, true);
    expect(commands.reportTrend).toHaveBeenCalledWith(
      { kind: "custom", from: "2026-10-10", to: "2026-11-08" },
      null,
      "day",
      true,
    );
    expect(commands.reportTrend).toHaveBeenCalledWith({ kind: "year", year: 2026 }, null, "month", false);
    await waitFor(() =>
      expect(within(board).getByRole("figure", { name: "Sales trend" })).toBeInTheDocument(),
    );
    expect(within(board).getByRole("figure", { name: "Payment methods" })).toBeInTheDocument();
    expect(within(board).getByRole("figure", { name: "Revenue and expenses" })).toBeInTheDocument();
  });

  it("leaves out gross profit without cost permissions, and top customers without CUSTOMER_VIEW", async () => {
    vi.mocked(commands.reportSummary).mockImplementation(() =>
      ok({ current: summary(1_000_000, null), previous: null }),
    );
    renderWithProviders(<Dashboard />);
    const board = await screen.findByRole("region", { name: "Today" });
    await within(board).findByText("Net sales");
    expect(within(board).queryByText("Gross profit")).not.toBeInTheDocument();
    expect(within(board).queryByText(/vs yesterday/)).not.toBeInTheDocument();
    expect(commands.customerAnalytics).not.toHaveBeenCalled();
  });

  it("does calendar arithmetic across months and years", () => {
    expect(addDays("2026-11-08", -29)).toBe("2026-10-10");
    expect(addDays("2026-01-01", -1)).toBe("2025-12-31");
    expect(addDays("2028-02-28", 1)).toBe("2028-02-29");
  });
});
