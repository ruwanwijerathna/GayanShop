import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, type Category_Serialize as Category, type Perm, type Product_Serialize as Product } from "@/db/bindings";
import { ok, renderWithProviders, session } from "@/test/render";
import { ProductsStockPage } from "./ProductsStockPage";
import { useSession } from "./session";

vi.mock("@/db/bindings", () => ({
  commands: {
    listProducts: vi.fn(),
    listCategories: vi.fn(),
    appStatus: vi.fn(),
    labelPrinterProfile: vi.fn(),
    exportPickFolder: vi.fn(),
    exportCsv: vi.fn(),
    verifyOwnerPin: vi.fn(),
    clearProductStock: vi.fn(),
  },
}));

const categories: Category[] = [
  { id: "c1", name: "Home", parent_id: null, product_count: 0 },
  { id: "c2", name: "Kitchen", parent_id: "c1", product_count: 0 },
];

const testProducts: Product[] = [
  {
    id: "p1",
    kind: "standard",
    name: "Item 1",
    name_si: null,
    sku: "SKU1",
    barcode: "BC1",
    category_id: "c1",
    default_cost: 100,
    price: 200,
    min_price: null,
    track_stock: true,
    allow_negative_stock: false,
    attributes: {},
    is_active: true,
    barcode_conflict: false,
    created_at: "2026-11-01T00:00:00Z",
    updated_at: "2026-11-01T00:00:00Z",
    version: 1,
    on_hand: 10,
  },
];

function As({ perms, children }: { perms: Perm[]; children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: perms })), [setSession, perms]);
  return <>{children}</>;
}

beforeEach(() => {
  vi.mocked(commands.listProducts).mockImplementation(() => ok(testProducts));
  vi.mocked(commands.listCategories).mockImplementation(() => ok(categories));
  vi.mocked(commands.appStatus).mockImplementation(() => ok({ device_code: "T02" } as never));
  vi.mocked(commands.labelPrinterProfile).mockImplementation(() => ok(null));
  vi.mocked(commands.verifyOwnerPin).mockImplementation(() => ok(null));
  vi.mocked(commands.clearProductStock).mockImplementation(() => ok(1));
});

describe("ProductsStockPage", () => {
  it("opens Categories from the tools menu, not inside Export & Import", async () => {
    renderWithProviders(
      <As perms={["PRODUCT_MANAGE", "DATA_EXPORT"]}>
        <ProductsStockPage />
      </As>,
    );
    expect(await screen.findByRole("button", { name: "Add product" })).toBeInTheDocument(); // the slider's handle
    expect(screen.queryByRole("button", { name: /^Categories/ })).not.toBeInTheDocument();

    fireEvent.keyDown(screen.getByRole("button", { name: "Products & Stock tools" }), { key: "Enter" });
    const catItem = await screen.findByRole("menuitem", { name: "Categories" });
    fireEvent.click(catItem);
    const dialog = await screen.findByRole("dialog", { name: "Categories" });
    expect(within(dialog).getByText("Kitchen")).toBeInTheDocument();
  });

  it("opens Clear all stock from the tools menu and requires Owner PIN", async () => {
    renderWithProviders(
      <As perms={["PRODUCT_MANAGE"]}>
        <ProductsStockPage />
      </As>,
    );
    fireEvent.keyDown(screen.getByRole("button", { name: "Products & Stock tools" }), { key: "Enter" });
    const clearAllItem = await screen.findByRole("menuitem", { name: "Clear all stock" });
    fireEvent.click(clearAllItem);

    const pinDialog = await screen.findByRole("dialog", { name: "Clear all stock" });
    expect(within(pinDialog).getByLabelText("PIN")).toBeInTheDocument();

    const pinInput = within(pinDialog).getByLabelText("PIN");
    fireEvent.change(pinInput, { target: { value: "1234" } });
    fireEvent.click(within(pinDialog).getByRole("button", { name: "Confirm" }));

    await waitFor(() => expect(commands.verifyOwnerPin).toHaveBeenCalledWith("1234"));
    await waitFor(() => expect(commands.clearProductStock).toHaveBeenCalledWith(["p1"]));
  });
});
