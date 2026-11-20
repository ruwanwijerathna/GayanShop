import { render, screen, within } from "@testing-library/react";
import type { ReactElement } from "react";
import { I18nextProvider } from "react-i18next";
import { beforeAll, describe, expect, it } from "vitest";
import { createI18n, type UiLanguage } from "@/i18n";
import { BatchSellThroughBar } from "./BatchSellThroughBar";
import { axisMoney, bucketLabel, chartColor, percentBp, tokenConfig, tooltipMoney } from "./format";
import { PaymentDonut } from "./PaymentDonut";
import { RevenueExpenseBar } from "./RevenueExpenseBar";
import { SalesTrendArea, overlay } from "./SalesTrendArea";
import { StaffDailySalesBar } from "./StaffDailySalesBar";
import { TopCustomersBar } from "./TopCustomersBar";

beforeAll(() => {
  // jsdom has no layout engine: Recharts' ResponsiveContainer needs a ResizeObserver and a size.
  class StubResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  globalThis.ResizeObserver = StubResizeObserver;
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 640 });
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 256 });
});

function renderIn(lang: UiLanguage, ui: ReactElement) {
  return render(<I18nextProvider i18n={createI18n(lang)}>{ui}</I18nextProvider>);
}

/** The visually hidden table every chart carries: its header and body cells, row by row. */
function tableOf(name: string): string[][] {
  const figure = screen.getByRole("figure", { name });
  const table = within(figure).getByRole("table");
  return within(table)
    .getAllByRole("row")
    .map((r) => [...r.querySelectorAll("th, td")].map((c) => c.textContent ?? ""));
}

const TREND = [
  { bucket: "2026-11-01", net_sales: 120_000, expenses: 30_000 },
  { bucket: "2026-11-02", net_sales: 0, expenses: 0 },
  { bucket: "2026-11-03", net_sales: 245_050, expenses: 10_000 },
];

describe("chart formatting", () => {
  it("formats money axes, shares and buckets", () => {
    expect(axisMoney(1_234_550)).toBe("Rs 12,346");
    expect(axisMoney(0)).toBe("Rs 0");
    expect(percentBp(7_550)).toBe("75.5%");
    expect(percentBp(-250)).toBe("-2.5%");
    expect(bucketLabel("2026-11-08")).toBe("08 Nov");
    expect(bucketLabel("2026-01")).toBe("Jan 2026");
    expect(tooltipMoney(12_345)).toBe("Rs 123.45");
    expect(tooltipMoney("n/a")).toBe("n/a");
  });

  it("takes every colour from the theme's chart tokens (light and dark)", () => {
    const config = tokenConfig({ a: { label: "A", n: 1 }, b: { label: "B", n: 5 } });
    expect(Object.values(config).map((c) => c.color)).toEqual(["var(--chart-1)", "var(--chart-5)"]);
    expect(chartColor(3)).toBe("var(--chart-3)");
    expect(JSON.stringify(config)).not.toMatch(/#[0-9a-f]{3,8}/i);
  });

  it("pairs the previous period point by point", () => {
    expect(overlay(TREND, [{ bucket: "2026-10-01", net_sales: 5 }])).toEqual([
      { bucket: "01 Nov", current: 120_000, previous: 5 },
      { bucket: "02 Nov", current: 0, previous: null },
      { bucket: "03 Nov", current: 245_050, previous: null },
    ]);
    expect(overlay(TREND, null)[0]?.previous).toBeNull();
  });
});

describe("charts", () => {
  it("revenue vs expenses, in English and Sinhala", () => {
    const { unmount } = renderIn("en", <RevenueExpenseBar points={TREND} />);
    expect(tableOf("Revenue and expenses")).toEqual([
      ["Period", "Revenue", "Expenses"],
      ["01 Nov", "Rs 1,200.00", "Rs 300.00"],
      ["02 Nov", "Rs 0.00", "Rs 0.00"],
      ["03 Nov", "Rs 2,450.50", "Rs 100.00"],
    ]);
    unmount();
    renderIn("si", <RevenueExpenseBar points={TREND} />);
    expect(tableOf("ආදායම සහ වියදම්")[0]).toEqual(["කාලය", "ආදායම", "වියදම්"]);
  });

  it("shows revenue alone when expenses are hidden by permissions", () => {
    renderIn("en", <RevenueExpenseBar points={TREND.map((p) => ({ ...p, expenses: null }))} />);
    expect(tableOf("Revenue and expenses")[0]).toEqual(["Period", "Revenue"]);
  });

  it("sales trend with the previous period overlaid", () => {
    renderIn(
      "en",
      <SalesTrendArea
        current={TREND}
        previous={[
          { bucket: "2026-10-01", net_sales: 100_000 },
          { bucket: "2026-10-02", net_sales: 50_000 },
          { bucket: "2026-10-03", net_sales: 0 },
        ]}
      />,
    );
    expect(tableOf("Sales trend")).toEqual([
      ["Period", "This period", "Previous period"],
      ["01 Nov", "Rs 1,200.00", "Rs 1,000.00"],
      ["02 Nov", "Rs 0.00", "Rs 500.00"],
      ["03 Nov", "Rs 2,450.50", "Rs 0.00"],
    ]);
  });

  it("payment donut with shares, leaving out empty methods", () => {
    const { unmount } = renderIn(
      "en",
      <PaymentDonut
        payments={[
          { method: "cash", amount: 750_000, count: 30 },
          { method: "card", amount: 250_000, count: 5 },
          { method: "qr", amount: 0, count: 0 },
        ]}
      />,
    );
    expect(tableOf("Payment methods")).toEqual([
      ["Method", "Amount", "Share", "Payments"],
      ["Cash", "Rs 7,500.00", "75.0%", "30"],
      ["Card", "Rs 2,500.00", "25.0%", "5"],
    ]);
    unmount();
    renderIn("si", <PaymentDonut payments={[{ method: "cash", amount: 100, count: 1 }]} />);
    expect(tableOf("ගෙවීම් ක්‍රම")[1]).toEqual(["මුදල්", "Rs 1.00", "100.0%", "1"]);
  });

  it("top customers by spend", () => {
    renderIn(
      "en",
      <TopCustomersBar
        customers={[
          { id: "c1", label: "Nimal", visits: 3, spend: 125_000 },
          { id: "c2", label: "077 123 4567", visits: 1, spend: 40_000 },
        ]}
      />,
    );
    expect(tableOf("Top customers by spend")).toEqual([
      ["Customer", "Visits", "Spend"],
      ["Nimal", "3", "Rs 1,250.00"],
      ["077 123 4567", "1", "Rs 400.00"],
    ]);
  });

  it("batch sell-through, Sinhala headers", () => {
    renderIn(
      "si",
      <BatchSellThroughBar
        batches={[
          {
            batch_id: "b1",
            code: "B-2026-1001",
            received: 100,
            sold: 62,
            written_off: 3,
            on_hand: 35,
            sell_through_bp: 6_200,
          },
        ]}
      />,
    );
    expect(tableOf("කාණ්ඩ විකිණීමේ ප්‍රගතිය")).toEqual([
      ["කාණ්ඩය", "ලැබුණු", "විකුණූ", "ලියා හැරි", "ඉතිරි", "විකිණුම් අනුපාතය"],
      ["B-2026-1001", "100", "62", "3", "35", "62.0%"],
    ]);
  });

  it("staff daily sales, English and Sinhala, quiet days as zero", () => {
    const days = [
      { date: "2026-11-06", total: 0, orders: 0 },
      { date: "2026-11-07", total: 120_050, orders: 3 },
    ];
    const en = renderIn("en", <StaffDailySalesBar days={days} />);
    expect(tableOf("Daily sales (last 30 days)")).toEqual([
      ["Date", "Sales", "Sales count"],
      ["06 Nov", "Rs 0.00", "0"],
      ["07 Nov", "Rs 1,200.50", "3"],
    ]);
    en.unmount();
    renderIn("si", <StaffDailySalesBar days={days} />);
    expect(tableOf("දෛනික විකුණුම් (පසුගිය දින 30)")[0]).toEqual(["දිනය", "විකුණුම්", "විකුණුම් ගණන"]);
  });

  it("says so when there is nothing to draw", () => {
    renderIn("en", <PaymentDonut payments={[]} />);
    const figure = screen.getByRole("figure", { name: "Payment methods" });
    expect(within(figure).getByText("No data for this period.")).toBeInTheDocument();
    expect(within(figure).queryByRole("table")).not.toBeInTheDocument();
  });
});
