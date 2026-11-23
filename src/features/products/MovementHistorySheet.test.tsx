import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  commands,
  type MovementRow_Serialize as MovementRow,
  type Product_Serialize as Product,
} from "@/db/bindings";
import { err, ok, renderWithProviders } from "@/test/render";
import { MovementHistorySheet } from "./MovementHistorySheet";

vi.mock("@/db/bindings", () => ({
  commands: { stockMovements: vi.fn() },
}));

const POT: Product = {
  id: "pot",
  kind: "standard",
  name: "Steel Pot",
  name_si: null,
  sku: "100001",
  barcode: "2100000000017",
  category_id: null,
  default_cost: 6_000,
  price: 9_000,
  min_price: null,
  track_stock: true,
  allow_negative_stock: true,
  attributes: {},
  is_active: true,
  barcode_conflict: false,
  created_at: "2026-11-08T09:00:00.000Z",
  updated_at: "2026-11-08T09:00:00.000Z",
  version: 1,
  on_hand: 7,
};

function row(over: Partial<MovementRow> & Pick<MovementRow, "id" | "reason" | "qty_delta">): MovementRow {
  return {
    created_at: "2026-11-08T09:00:00.000Z",
    business_date: "2026-11-08",
    unit_cost: 6_000,
    batch_item_id: "bi1",
    batch_code: "B-2026-1001",
    ref_type: null,
    ref_id: null,
    note: null,
    staff_name: "Owner",
    approved_by_name: null,
    ...over,
  };
}

const NEWEST = [
  row({ id: "m3", reason: "damage", qty_delta: -1, note: "cracked", approved_by_name: "Kamal" }),
  row({ id: "m2", reason: "sale", qty_delta: -2 }),
];
const OLDEST = [row({ id: "m1", reason: "batch_intake", qty_delta: 10, business_date: "2026-11-07" })];

beforeEach(() => {
  vi.mocked(commands.stockMovements).mockReset();
  vi.mocked(commands.stockMovements)
    .mockImplementationOnce(() => ok({ rows: NEWEST, next_cursor: ["2026-11-08T09:00:00.000Z", "m2"] }))
    .mockImplementationOnce(() => ok({ rows: OLDEST, next_cursor: null }));
});

describe("MovementHistorySheet", () => {
  it("shows totals, days, details and the balance each movement left, and loads older movements", async () => {
    renderWithProviders(<MovementHistorySheet product={POT} onClose={() => undefined} />);
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText("cracked")).toBeInTheDocument();
    expect(within(dialog).getByText("Steel Pot")).toBeInTheDocument();
    expect(within(dialog).getByText("Approved by Kamal")).toBeInTheDocument();
    expect(within(dialog).getAllByText("Batch B-2026-1001")).toHaveLength(2);
    expect(within(dialog).getByText("Value Rs 60.00")).toBeInTheDocument();
    // today's 7, wound back through the newer movements
    expect(within(dialog).getByText("Balance 7")).toBeInTheDocument();
    expect(within(dialog).getByText("Balance 8")).toBeInTheDocument();
    expect(within(dialog).getByText("Net -3")).toBeInTheDocument();
    expect(within(dialog).getByText(/most recent movements/)).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: "Load older movements" }));
    expect(await within(dialog).findByText("Balance 10")).toBeInTheDocument();
    expect(within(dialog).getByText("Net +10")).toBeInTheDocument();
    expect(within(dialog).getByText("+10", { selector: "div.text-lg" })).toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "Load older movements" })).not.toBeInTheDocument();
    expect(commands.stockMovements).toHaveBeenLastCalledWith(
      "pot",
      null,
      { created_at: "2026-11-08T09:00:00.000Z", id: "m2" },
      100,
    );
  });

  it("filters the timeline by movement type", async () => {
    renderWithProviders(<MovementHistorySheet product={POT} onClose={() => undefined} />);
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByText("cracked");
    fireEvent.click(within(dialog).getByRole("button", { name: /^Sold/ }));
    expect(within(dialog).queryByText("cracked")).not.toBeInTheDocument();
    expect(within(dialog).getByText("Balance 8")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: /^All/ }));
    expect(within(dialog).getByText("cracked")).toBeInTheDocument();
  });

  it("explains an empty ledger", async () => {
    vi.mocked(commands.stockMovements).mockReset();
    vi.mocked(commands.stockMovements).mockImplementation(() => ok({ rows: [], next_cursor: null }));
    renderWithProviders(<MovementHistorySheet product={POT} onClose={() => undefined} />);
    expect(await screen.findByText("No movements yet.")).toBeInTheDocument();
  });

  it("shows Rust's refusal in the user's language", async () => {
    vi.mocked(commands.stockMovements).mockReset();
    vi.mocked(commands.stockMovements).mockImplementation(() =>
      err({ kind: "FORBIDDEN", code: "FORBIDDEN", detail: "" } as never),
    );
    renderWithProviders(<MovementHistorySheet product={POT} onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByRole("alert")).toBeInTheDocument());
  });
});
