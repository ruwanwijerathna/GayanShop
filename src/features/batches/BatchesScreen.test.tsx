import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
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
    updateBatch: vi.fn(),
    batchAllocationPreview: vi.fn(),
    reopenBatch: vi.fn(),
    closeBatch: vi.fn(),
    addLateBatchItem: vi.fn(),
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
    landed_cost: 0,
    allocation_method: "manual",
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
  pnl: null,
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

beforeAll(() => {
  // jsdom has no layout: give the virtualised lists a viewport.
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 576 });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 1000 });
});

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
  vi.mocked(commands.batchAllocationPreview).mockImplementation(() =>
    ok({
      method: "manual",
      landed_cost: 210_000,
      allocated: 200_000,
      variance: 10_000,
      lines: [
        {
          item_id: "bi1",
          product_name: "Blue Bucket",
          qty: 10,
          retail_price: 45_000,
          unit_cost: 20_000,
          line_cost: 200_000,
        },
      ],
      problem: null,
    }),
  );
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

describe("intake wizard and lifecycle", () => {
  it("shows the live allocation and its variance, and changes the method", async () => {
    vi.mocked(commands.updateBatch).mockImplementation(() => ok(draft));
    const detail = await openDraft();
    expect(await within(detail).findByTestId("variance")).toHaveTextContent("Rs 100.00");
    fireEvent.change(within(detail).getByLabelText("How to share the cost"), {
      target: { value: "by_retail_value" },
    });
    fireEvent.change(within(detail).getByLabelText("Invoice total (Rs, optional)"), {
      target: { value: "2000" },
    });
    fireEvent.click(within(detail).getByRole("button", { name: "Save details" }));
    await waitFor(() =>
      expect(commands.updateBatch).toHaveBeenCalledWith(
        "b1",
        expect.objectContaining({
          allocation_method: "by_retail_value",
          total_cost: 200_000,
          extra_costs: 0,
        }),
      ),
    );
  });

  it("explains why receive would be refused", async () => {
    vi.mocked(commands.batchAllocationPreview).mockImplementation(() =>
      ok({
        method: "even_per_unit",
        landed_cost: 0,
        allocated: 0,
        variance: 0,
        lines: [],
        problem: "BATCH_TOTAL_REQUIRED",
      }),
    );
    const detail = await openDraft();
    expect(await within(detail).findByText(/Enter the invoice total/)).toBeInTheDocument();
  });

  it("shows a received batch's P&L and reopens or closes it after confirmation", async () => {
    const received = {
      ...draft,
      batch: batch({ status: "received", received_at: "2026-11-08T09:30:00.000Z", landed_cost: 210_000 }),
      pnl: {
        landed_cost: 210_000,
        units_received: 10,
        units_sold: 4,
        units_remaining: 6,
        revenue_net: 180_000,
        cogs: 84_000,
        written_off_qty: 0,
        written_off_cost: 0,
        allocation_variance: 0,
        remaining_value_cost: 126_000,
        remaining_value_retail: 270_000,
        realised_profit: 96_000,
        payback: -30_000,
        projected_profit: 240_000,
        sell_through_bp: 4_000,
      },
    };
    vi.mocked(commands.getBatch).mockImplementation(() => ok(received));
    vi.mocked(commands.reopenBatch).mockImplementation(() =>
      err({ kind: "VALIDATION", code: "BATCH_HAS_MOVEMENTS", detail: "B-2026-1001" }),
    );
    vi.mocked(commands.closeBatch).mockImplementation(() =>
      err({ kind: "VALIDATION", code: "BATCH_HAS_STOCK", detail: "6" }),
    );
    const detail = await openDraft();
    expect(within(detail).getByText("Did this batch make money?")).toBeInTheDocument();
    expect(within(detail).getByText(/10 received · 4 sold · 6 left \(40% sold\)/)).toBeInTheDocument();
    expect(within(detail).getByText("-Rs 300.00")).toBeInTheDocument();

    fireEvent.click(within(detail).getByRole("button", { name: "Reopen to fix" }));
    expect(commands.reopenBatch).not.toHaveBeenCalled();
    fireEvent.click(within(detail).getByRole("button", { name: "Yes, reopen" }));
    await waitFor(() => expect(commands.reopenBatch).toHaveBeenCalledWith("b1"));
    expect(await within(detail).findByText(/already sold or moved/)).toBeInTheDocument();

    fireEvent.click(within(detail).getByRole("button", { name: "Close batch" }));
    fireEvent.click(within(detail).getByRole("button", { name: "Yes, close" }));
    expect(await within(detail).findByText(/6 unit\(s\) are still on hand/)).toBeInTheDocument();
  });

  it("adds a late item to a received batch", async () => {
    vi.mocked(commands.getBatch).mockImplementation(() =>
      ok({ ...draft, batch: batch({ status: "received", received_at: "2026-11-08T09:30:00.000Z" }) }),
    );
    vi.mocked(commands.addLateBatchItem).mockImplementation(() => ok(draft));
    const detail = await openDraft();
    const form = within(detail).getByRole("form", { name: "Add a forgotten item" });
    await pick(form);
    fireEvent.change(within(form).getByLabelText("Quantity"), { target: { value: "5" } });
    fireEvent.click(within(form).getByRole("button", { name: "Add late item" }));
    await waitFor(() =>
      expect(commands.addLateBatchItem).toHaveBeenCalledWith("b1", {
        product_id: "p2",
        qty: 5,
        retail_price: null,
      }),
    );
  });

  it("starts a new batch with cost shared per unit by default", async () => {
    vi.mocked(commands.createBatch).mockImplementation(() => ok(draft));
    renderWithProviders(<BatchesScreen />);
    fireEvent.click(await screen.findByRole("button", { name: "New batch" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Batch name"), { target: { value: "Shoes" } });
    fireEvent.change(within(dialog).getByLabelText("Invoice total (Rs, optional)"), {
      target: { value: "5000" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(commands.createBatch).toHaveBeenCalledWith(
        expect.objectContaining({ name: "Shoes", total_cost: 500_000, allocation_method: "even_per_unit" }),
      ),
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
