import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import { commands, type Perm, type ProductSummary_Serialize as ProductSummary } from "@/db/bindings";
import { err, ok, renderWithProviders, session } from "@/test/render";
import { InventoryScreen, parseUnits } from "./InventoryScreen";

vi.mock("@/db/bindings", () => ({
  commands: {
    stockValuation: vi.fn(),
    oversoldLayers: vi.fn(),
    stockMovements: vi.fn(),
    searchProducts: vi.fn(),
    recordDamage: vi.fn(),
    adjustStock: vi.fn(),
    recordStockCount: vi.fn(),
    listLoginStaff: vi.fn(),
    requestApproval: vi.fn(),
  },
}));

const POT: ProductSummary = {
  id: "pot",
  kind: "standard",
  name: "Pot",
  name_si: null,
  sku: "100001",
  barcode: null,
  price: 50_000,
  min_price: null,
  track_stock: true,
};

const CHANGE = { product_id: "pot", qty_delta: -2, cost_delta: -60_000, on_hand_after: 2 };

function As({ perms, children }: { perms: Perm[]; children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: perms })), [setSession, perms]);
  return <>{children}</>;
}

const OWNER: Perm[] = ["SALE_CREATE", "STOCK_DAMAGE", "STOCK_ADJUST", "REPORT_FINANCIAL"];

function renderAs(perms: Perm[] = OWNER) {
  renderWithProviders(
    <As perms={perms}>
      <InventoryScreen />
    </As>,
  );
}

async function pickPot(label: string) {
  fireEvent.change(await screen.findByLabelText(label), { target: { value: "pot" } });
  fireEvent.click(await screen.findByRole("button", { name: /Pot/ }));
}

beforeAll(() => {
  // jsdom has no layout: give the virtualised history a viewport.
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 480 });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 1000 });
});

beforeEach(() => {
  vi.mocked(commands.searchProducts).mockImplementation(() => ok([POT]));
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
  vi.mocked(commands.recordDamage).mockReset();
  vi.mocked(commands.adjustStock).mockReset();
  vi.mocked(commands.recordStockCount).mockReset();
  vi.mocked(commands.listLoginStaff).mockImplementation(() =>
    ok([
      { id: "cashier", display_name: "Nimal", role_name: "Cashier" },
      { id: "sup", display_name: "Kamal", role_name: "Supervisor" },
    ]),
  );
  vi.mocked(commands.requestApproval).mockImplementation(() => ok({ approval_id: "appr", expires_at: "" }));
});

describe("InventoryScreen", () => {
  it("pages a product's movement history on the virtualised table", async () => {
    const row = (id: string, reason: string, qty: number) => ({
      id,
      created_at: "2026-11-08T09:00:00.000Z",
      business_date: "2026-11-08",
      reason,
      qty_delta: qty,
      unit_cost: 6_000,
      batch_item_id: "bi1",
      batch_code: "B-2026-1001",
      ref_type: null,
      ref_id: null,
      note: reason === "damage" ? "cracked" : null,
      staff_name: "Owner",
      approved_by_name: null,
    });
    vi.mocked(commands.stockMovements)
      .mockImplementationOnce(() =>
        ok({
          rows: [row("m3", "damage", -1), row("m2", "sale", -2)],
          next_cursor: ["2026-11-08T09:00:00.000Z", "m2"],
        }),
      )
      .mockImplementationOnce(() => ok({ rows: [row("m1", "batch_intake", 10)], next_cursor: null }));
    renderAs();
    fireEvent.click(screen.getByRole("tab", { name: /History/ }));
    await pickPot("Product");
    const grid = await screen.findByRole("grid", { name: /History/ });
    expect(await within(grid).findByText("cracked")).toBeInTheDocument();
    await waitFor(() => expect(commands.stockMovements).toHaveBeenCalledTimes(2));
    expect(commands.stockMovements).toHaveBeenLastCalledWith(
      "pot",
      null,
      { created_at: "2026-11-08T09:00:00.000Z", id: "m2" },
      100,
    );
    expect(await within(grid).findByText("10")).toBeInTheDocument();
  });

  it("values the stock now and as of a date, flagging oversold layers", async () => {
    renderAs();
    fireEvent.click(screen.getByRole("tab", { name: /Valuation/ }));
    expect(await screen.findByTestId("value-at-cost")).toHaveTextContent("Rs 1,250.00");
    expect(screen.getByText("No category")).toBeInTheDocument();
    expect(screen.getByText(/1 oversold layer/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("As of (business date)"), { target: { value: "2026-11-08" } });
    await waitFor(() => expect(commands.stockValuation).toHaveBeenLastCalledWith("2026-11-08"));
  });

  it("lists oversold layers", async () => {
    renderAs();
    fireEvent.click(screen.getByRole("tab", { name: /Oversold/ }));
    const row = (await screen.findByText("Cup")).closest("tr");
    if (row) expect(within(row).getByText("-2")).toBeInTheDocument();
  });

  it("writes off damage, asking a supervisor when needed", async () => {
    vi.mocked(commands.recordDamage)
      .mockImplementationOnce(() => err({ kind: "APPROVAL_REQUIRED", perm: "STOCK_DAMAGE" }))
      .mockImplementationOnce(() => ok(CHANGE));
    renderAs(["SALE_CREATE"]);
    await pickPot("Product");
    fireEvent.change(screen.getByLabelText("Units"), { target: { value: "2" } });
    fireEvent.change(screen.getByLabelText("Why"), { target: { value: "expired" } });
    fireEvent.click(screen.getByRole("button", { name: "Write off" }));
    fireEvent.click(await screen.findByRole("button", { name: "Kamal" }));
    for (const d of "2222") fireEvent.click(screen.getByRole("button", { name: `Digit ${d}` }));
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    await waitFor(() => expect(commands.recordDamage).toHaveBeenCalledTimes(2));
    expect(commands.requestApproval).toHaveBeenCalledWith("STOCK_DAMAGE", "stock", "sup", "2222");
    expect(commands.recordDamage).toHaveBeenLastCalledWith(
      { product_id: "pot", qty: 2, reason: "expired", note: null },
      "appr",
    );
  });

  it("adjusts with a signed quantity and a required reason", async () => {
    vi.mocked(commands.adjustStock).mockImplementation(() => ok({ ...CHANGE, qty_delta: 3 }));
    renderAs();
    fireEvent.click(screen.getByRole("button", { name: "Adjust (+ found / - removed)" }));
    await pickPot("Product");
    fireEvent.change(screen.getByLabelText("Units (+ or -)"), { target: { value: "3" } });
    expect(screen.getByRole("button", { name: "Adjust stock" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Reason (required)"), {
      target: { value: "found in store room" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Adjust stock" }));
    await waitFor(() =>
      expect(commands.adjustStock).toHaveBeenCalledWith(
        { product_id: "pot", qty_delta: 3, note: "found in store room" },
        null,
      ),
    );
  });

  it("saves a stock count", async () => {
    vi.mocked(commands.recordStockCount).mockImplementation(() => ok({ changes: [CHANGE], unchanged: 0 }));
    renderAs();
    fireEvent.click(screen.getByRole("tab", { name: /Stock count/ }));
    await pickPot("Add a product to the count");
    fireEvent.change(screen.getByLabelText("Counted Pot"), { target: { value: "2" } });
    fireEvent.click(screen.getByRole("button", { name: "Save count (1)" }));
    await waitFor(() =>
      expect(commands.recordStockCount).toHaveBeenCalledWith(
        { lines: [{ product_id: "pot", counted: 2 }], note: null },
        null,
      ),
    );
  });

  it("parses units", () => {
    expect(parseUnits("12")).toBe(12);
    expect(parseUnits("-3")).toBeNull();
    expect(parseUnits("-3", true)).toBe(-3);
    expect(parseUnits("1.5", true)).toBeNull();
  });
});
