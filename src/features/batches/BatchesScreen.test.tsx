import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  commands,
  type BatchDetail_Serialize as BatchDetail,
  type Batch_Serialize as Batch,
  type ProductSummary_Serialize as ProductSummary,
} from "@/db/bindings";
import { err, ok, renderWithProviders } from "@/test/render";
import { parseOptionalMoney, parseQty } from "./batchForm";
import { BatchesScreen } from "./BatchesScreen";

vi.mock("@/db/bindings", () => ({
  commands: {
    listBatches: vi.fn(),
    getBatch: vi.fn(),
    createBatch: vi.fn(),
    setBatchItem: vi.fn(),
    removeBatchItem: vi.fn(),
    receiveBatch: vi.fn(),
    adHocPurchase: vi.fn(),
    searchProducts: vi.fn(),
  },
}));

function batch(over: Partial<Batch>): Batch {
  return {
    id: "b1",
    code: "B-2026-1001",
    name: "Pettah 8 Nov",
    supplier_name: "Pettah wholesale",
    supplier_ref: null,
    purchased_at: "2026-11-08T09:00:00.000Z",
    received_at: null,
    total_cost: 0,
    extra_costs: 0,
    allocation_variance: 0,
    status: "draft",
    is_system: false,
    notes: null,
    item_count: 1,
    total_units: 10,
    items_cost: 200_000,
    version: 1,
    ...over,
  };
}

const draft: BatchDetail = {
  batch: batch({}),
  items: [
    {
      id: "bi1",
      product_id: "p1",
      product_name: "Blue Bucket",
      product_sku: "100001",
      qty_received: 10,
      unit_cost: 20_000,
      retail_price_at_intake: 45_000,
      added_after_receipt: false,
      on_hand: 0,
      sold_qty: 0,
    },
  ],
};

const bucket: ProductSummary = {
  id: "p2",
  kind: "standard",
  name: "Red Basin",
  name_si: null,
  sku: "100002",
  barcode: null,
  price: 30_000,
  min_price: null,
  track_stock: true,
};

beforeEach(() => {
  vi.mocked(commands.listBatches).mockImplementation(() =>
    ok([
      batch({}),
      batch({ id: "os", code: "OPENING-STOCK", name: "Opening stock", is_system: true, total_units: 0 }),
      batch({
        id: "adj",
        code: "ADJUSTMENTS",
        name: "Stock adjustments",
        is_system: true,
        status: "received",
      }),
    ]),
  );
  vi.mocked(commands.getBatch).mockImplementation(() => ok(draft));
  vi.mocked(commands.searchProducts).mockImplementation(() => ok([bucket]));
  vi.mocked(commands.setBatchItem).mockReset();
  vi.mocked(commands.receiveBatch).mockReset();
  vi.mocked(commands.adHocPurchase).mockReset();
});

async function openDraft() {
  renderWithProviders(<BatchesScreen />);
  fireEvent.click(await screen.findByRole("button", { name: /Pettah 8 Nov/ }));
  return screen.findByRole("region", { name: /Pettah 8 Nov/ });
}

async function pick(container: HTMLElement) {
  fireEvent.change(within(container).getByRole("searchbox", { name: "Find a product" }), {
    target: { value: "bas" },
  });
  fireEvent.click(await within(container).findByRole("button", { name: /Red Basin/ }));
}

describe("BatchesScreen", () => {
  it("lists user batches and opening stock, but hides the internal ADJUSTMENTS batch", async () => {
    renderWithProviders(<BatchesScreen />);
    expect(await screen.findByRole("button", { name: /Pettah 8 Nov/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Opening stock/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Stock adjustments/ })).not.toBeInTheDocument();
  });

  it("adds a product line to a draft with integer quantity and cents", async () => {
    vi.mocked(commands.setBatchItem).mockImplementation(() => ok(draft));
    const detail = await openDraft();
    expect(within(detail).getByText("Blue Bucket")).toBeInTheDocument();
    await pick(detail);
    fireEvent.change(within(detail).getByLabelText("Quantity"), { target: { value: "12" } });
    fireEvent.change(within(detail).getByLabelText("Cost per unit (Rs)"), { target: { value: "185.50" } });
    fireEvent.click(within(detail).getByRole("button", { name: "Add to batch" }));
    await waitFor(() =>
      expect(commands.setBatchItem).toHaveBeenCalledWith("b1", {
        product_id: "p2",
        qty: 12,
        unit_cost: 18_550,
        retail_price: null,
      }),
    );
  });

  it("refuses a bad quantity locally and shows Rust's reason for a refused line", async () => {
    vi.mocked(commands.setBatchItem).mockImplementation(() =>
      err({ kind: "VALIDATION", code: "PRODUCT_NOT_TRACKED", detail: "p2" }),
    );
    const detail = await openDraft();
    await pick(detail);
    fireEvent.change(within(detail).getByLabelText("Quantity"), { target: { value: "1.5" } });
    fireEvent.change(within(detail).getByLabelText("Cost per unit (Rs)"), { target: { value: "10" } });
    fireEvent.click(within(detail).getByRole("button", { name: "Add to batch" }));
    expect(await within(detail).findByRole("alert")).toHaveTextContent(
      "Enter a whole number of units, 1 or more.",
    );
    expect(commands.setBatchItem).not.toHaveBeenCalled();

    fireEvent.change(within(detail).getByLabelText("Quantity"), { target: { value: "2" } });
    fireEvent.click(within(detail).getByRole("button", { name: "Add to batch" }));
    expect(await within(detail).findByText(/doesn't track stock/)).toBeInTheDocument();
  });

  it("receives a draft only after confirmation", async () => {
    vi.mocked(commands.receiveBatch).mockImplementation(() =>
      ok({ ...draft, batch: batch({ status: "received", received_at: "2026-11-08T09:30:00.000Z" }) }),
    );
    const detail = await openDraft();
    fireEvent.click(within(detail).getByRole("button", { name: "Receive stock" }));
    expect(commands.receiveBatch).not.toHaveBeenCalled();
    expect(within(detail).getByText(/Receive 10 units\? Costs lock/)).toBeInTheDocument();
    fireEvent.click(within(detail).getByRole("button", { name: "Yes, receive" }));
    await waitFor(() => expect(commands.receiveBatch).toHaveBeenCalledWith("b1"));
  });

  it("records a quick one-item purchase", async () => {
    vi.mocked(commands.adHocPurchase).mockImplementation(() => ok(draft));
    renderWithProviders(<BatchesScreen />);
    fireEvent.click(await screen.findByRole("button", { name: "Quick purchase" }));
    const dialog = await screen.findByRole("dialog");
    await pick(dialog);
    fireEvent.change(within(dialog).getByLabelText("Quantity"), { target: { value: "6" } });
    fireEvent.change(within(dialog).getByLabelText("Cost per unit (Rs)"), { target: { value: "480" } });
    fireEvent.change(within(dialog).getByLabelText("Supplier (optional)"), { target: { value: "Market" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Receive stock" }));
    await waitFor(() =>
      expect(commands.adHocPurchase).toHaveBeenCalledWith({
        product_id: "p2",
        qty: 6,
        unit_cost: 48_000,
        supplier_name: "Market",
        note: null,
      }),
    );
  });
});

describe("batch form parsing", () => {
  it("accepts whole positive quantities only", () => {
    expect(parseQty("12")).toBe(12);
    expect(parseQty(" 3 ")).toBe(3);
    for (const bad of ["0", "", "1.5", "-2", "abc", "12345678"]) expect(parseQty(bad)).toBeNull();
  });

  it("treats an empty optional amount as zero and parses rupees to cents", () => {
    expect(parseOptionalMoney("")).toBe(0);
    expect(parseOptionalMoney("1,250.5")).toBe(125_050);
    expect(parseOptionalMoney("x")).toBeNull();
  });
});
