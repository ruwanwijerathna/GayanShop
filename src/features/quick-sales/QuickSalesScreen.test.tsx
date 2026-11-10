import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  commands,
  type Batch_Serialize as Batch,
  type QuickSaleRow_Serialize as QuickSaleRow,
} from "@/db/bindings";
import { chooseOption, ok, renderWithProviders } from "@/test/render";
import { QuickSalesScreen } from "./QuickSalesScreen";

vi.mock("@/db/bindings", () => ({
  commands: {
    quickSaleReview: vi.fn(),
    quickSaleDetail: vi.fn(),
    promoteQuickSale: vi.fn(),
    ignoreQuickSale: vi.fn(),
    listCategories: vi.fn(),
    listBatches: vi.fn(),
  },
}));

const ROW: QuickSaleRow = {
  custom_key: "blue bucket 10l",
  display_name: "Blue Bucket 10L",
  times_sold: 4,
  units_sold: 5,
  revenue_net: 225_000,
  last_price: 50_000,
  min_price: 40_000,
  max_price: 50_000,
  first_sold_at: "2026-11-08T09:00:00.000Z",
  last_sold_at: "2026-11-08T12:00:00.000Z",
  status: "new",
  product_id: null,
};

const BATCH = {
  id: "b1",
  code: "B-2026-1001",
  name: "Pettah",
  status: "received",
  is_system: false,
} as unknown as Batch;

beforeAll(() => {
  // jsdom has no layout: give the virtualised table a viewport.
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 480 });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 1000 });
});

beforeEach(() => {
  vi.mocked(commands.quickSaleReview).mockImplementation(() => ok([ROW]));
  vi.mocked(commands.quickSaleDetail).mockImplementation(() =>
    ok({
      custom_key: ROW.custom_key,
      median_price: 45_000,
      counters: [{ device_code: "T01", times_sold: 4, units: 5 }],
      spellings: ["Blue Bucket 10L", "blue bucket 10l"],
    }),
  );
  vi.mocked(commands.listCategories).mockImplementation(() => ok([]));
  vi.mocked(commands.listBatches).mockImplementation(() => ok([BATCH]));
  vi.mocked(commands.promoteQuickSale).mockReset();
  vi.mocked(commands.ignoreQuickSale).mockImplementation(() => ok(null));
});

describe("QuickSalesScreen", () => {
  it("lists keys to review, most sold first", async () => {
    renderWithProviders(<QuickSalesScreen />);
    const row = (await screen.findByRole("button", { name: "Blue Bucket 10L" })).closest<HTMLElement>(
      '[role="row"]',
    );
    expect(row).not.toBeNull();
    if (row) expect(within(row).getByText("Rs 400.00–Rs 500.00")).toBeInTheDocument();
    expect(commands.quickSaleReview).toHaveBeenCalledWith("new");
    fireEvent.click(screen.getByRole("button", { name: "All" }));
    await waitFor(() => expect(commands.quickSaleReview).toHaveBeenLastCalledWith(null));
  });

  it("promotes a key with its cost and shelf count in a received batch", async () => {
    vi.mocked(commands.promoteQuickSale).mockImplementation(() =>
      ok({ product: { name: "Blue Bucket 10L" } as never, shelf_qty: 6 }),
    );
    renderWithProviders(<QuickSalesScreen />);
    fireEvent.click(await screen.findByRole("button", { name: "Blue Bucket 10L" }));
    const form = await screen.findByRole("form", { name: "Promote to a product" });
    expect(await screen.findByText("Rs 450.00")).toBeInTheDocument();
    expect(within(form).getByLabelText("Selling price (Rs)")).toHaveValue("500.00");
    fireEvent.change(within(form).getByLabelText("Cost per unit (Rs)"), { target: { value: "200" } });
    fireEvent.change(within(form).getByLabelText("Units on the shelf now (optional)"), {
      target: { value: "6" },
    });
    chooseOption(within(form).getByLabelText("Batch the shelf stock belongs to"), /B-2026-1001/);
    fireEvent.click(within(form).getByRole("button", { name: "Promote" }));
    await waitFor(() =>
      expect(commands.promoteQuickSale).toHaveBeenCalledWith({
        custom_key: "blue bucket 10l",
        name: "Blue Bucket 10L",
        price: 50_000,
        default_cost: 20_000,
        category_id: null,
        generate_barcode: true,
        shelf_qty: 6,
        batch_id: "b1",
      }),
    );
  });

  it("ignores a key", async () => {
    renderWithProviders(<QuickSalesScreen />);
    fireEvent.click(await screen.findByRole("button", { name: "Blue Bucket 10L" }));
    fireEvent.click(await screen.findByRole("button", { name: "Ignore" }));
    await waitFor(() => expect(commands.ignoreQuickSale).toHaveBeenCalledWith("blue bucket 10l"));
  });
});
