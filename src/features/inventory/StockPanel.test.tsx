import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import { commands, type Perm } from "@/db/bindings";
import { ok, openTab, renderWithProviders, session } from "@/test/render";
import { StockPanel, type StockSection } from "./StockPanel";

const ALL: StockSection[] = ["valuation", "oversold"];

vi.mock("@/db/bindings", () => ({
  commands: {
    stockValuation: vi.fn(),
    oversoldLayers: vi.fn(),
  },
}));

function As({ perms, children }: { perms: Perm[]; children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: perms })), [setSession, perms]);
  return <>{children}</>;
}

function renderAs(perms: Perm[] = ["REPORT_FINANCIAL"]) {
  renderWithProviders(
    <As perms={perms}>
      <StockPanel sections={ALL} label="Stock" />
    </As>,
  );
}

beforeEach(() => {
  vi.mocked(commands.stockValuation).mockImplementation(() =>
    ok({
      as_of: null,
      total_units: 14,
      value_at_cost: 125_000,
      value_at_retail: 220_000,
      by_batch: [
        { id: "b1", label: "B-2026-1001 · Pots", units: 4, value_at_cost: 120_000, value_at_retail: 200_000 },
      ],
      by_category: [{ id: null, label: "", units: 10, value_at_cost: 5_000, value_at_retail: 20_000 }],
      negative_layers: 1,
    }),
  );
  vi.mocked(commands.oversoldLayers).mockImplementation(() =>
    ok([
      {
        batch_item_id: "bi",
        product_id: "cup",
        product_name: "Cup",
        batch_id: "b",
        batch_code: "B-1",
        on_hand: -2,
        unit_cost: 3_000,
      },
    ]),
  );
});

describe("StockPanel", () => {
  it("values the stock now and as of a date, flagging oversold layers", async () => {
    renderAs();
    expect(await screen.findByTestId("value-at-cost")).toHaveTextContent("Rs 1,250.00");
    expect(screen.getByText("No category")).toBeInTheDocument();
    expect(screen.getByText(/1 oversold layer/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("As of (business date)"), { target: { value: "2026-11-08" } });
    await waitFor(() => expect(commands.stockValuation).toHaveBeenLastCalledWith("2026-11-08"));
    fireEvent.click(screen.getByRole("button", { name: "Now" }));
    // Back to now (served from the cache of the first read), and Now itself turns off.
    expect(screen.getByLabelText("As of (business date)")).toHaveValue("");
    expect(screen.getByRole("button", { name: "Now" })).toBeDisabled();
  });

  it("shows the potential margin and totals by category", async () => {
    renderAs();
    // Retail 2,200.00 − cost 1,250.00 = 950.00, which is 43.1% of retail.
    expect(await screen.findByTestId("potential-margin")).toHaveTextContent("Rs 950.00");
    expect(screen.getByText("43.1% of today's prices, if all of it sells")).toBeInTheDocument();
    const byCategory = screen.getByRole("table", { name: "By category" });
    const total = within(byCategory).getByRole("rowheader", { name: "Total" }).closest("tr");
    expect(total).toHaveTextContent("14");
    expect(total).toHaveTextContent("Rs 1,250.00");
    expect(total).toHaveTextContent("Rs 2,200.00");
  });

  it("lists oversold layers", async () => {
    renderAs();
    openTab(await screen.findByRole("tab", { name: /Oversold/ }));
    const row = (await screen.findByText("Cup")).closest("tr");
    if (row) expect(within(row).getByText("-2")).toBeInTheDocument();
  });

  it("shows nothing to someone who may not see financial reports", () => {
    renderAs(["SALE_CREATE"]);
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
  });
});
