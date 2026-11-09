import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/db/bindings";
import { ok, renderWithProviders } from "@/test/render";
import { MaintenanceSettings } from "./MaintenanceSettings";

vi.mock("@/db/bindings", () => ({ commands: { rebuildDerived: vi.fn() } }));

beforeEach(() => {
  vi.mocked(commands.rebuildDerived).mockImplementation(() =>
    ok({
      daily_sales_summary: 120,
      daily_payment_summary: 300,
      customer_stats: 45,
      quick_sale_stats: 12,
      stock_levels: 800,
      batch_item_stock: 950,
      products_fts: 800,
    }),
  );
});

describe("MaintenanceSettings", () => {
  it("rebuilds the reports only after confirming, then says what it rebuilt", async () => {
    renderWithProviders(<MaintenanceSettings />);
    fireEvent.click(screen.getByRole("button", { name: "Rebuild reports" }));
    const dialog = await screen.findByRole("dialog");
    expect(commands.rebuildDerived).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Rebuild now" }));
    await waitFor(() => expect(commands.rebuildDerived).toHaveBeenCalledTimes(1));
    expect(
      await screen.findByText("Rebuilt 120 day totals, 45 customers and 950 stock layers."),
    ).toBeInTheDocument();
  });
});
