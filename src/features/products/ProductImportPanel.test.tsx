import { fireEvent, screen, waitFor } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import {
  commands,
  type Category_Serialize as Category,
  type Perm,
  type Product_Serialize as Product,
} from "@/db/bindings";
import { ok, renderWithProviders, session } from "@/test/render";
import { ProductImportPanel } from "./ProductImportPanel";

vi.mock("@/db/bindings", () => ({
  commands: {
    listCategories: vi.fn(),
    createCategory: vi.fn(),
    listProducts: vi.fn(),
    createProduct: vi.fn(),
    updateProduct: vi.fn(),
    adjustStock: vi.fn(),
  },
}));

const EXISTING_CATEGORIES: Category[] = [
  { id: "cat-1", name: "Footwear", parent_id: null, product_count: 5 },
];

function As({ perms, children }: { perms: Perm[]; children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: perms })), [setSession, perms]);
  return <>{children}</>;
}

describe("ProductImportPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.listCategories).mockImplementation(() => ok(EXISTING_CATEGORIES));
    vi.mocked(commands.createCategory).mockImplementation((input) =>
      ok({ id: "cat-new", name: input.name, parent_id: null, product_count: 0 }),
    );
    vi.mocked(commands.listProducts).mockImplementation(() => ok([]));
    vi.mocked(commands.createProduct).mockImplementation((input) =>
      ok({
        id: "prod-1",
        kind: "standard",
        name: input.name,
        name_si: input.name_si ?? null,
        sku: input.sku.mode === "manual" ? input.sku.value : null,
        barcode: input.barcode.mode === "manual" ? input.barcode.value : null,
        category_id: input.category_id ?? null,
        default_cost: input.default_cost ?? null,
        price: input.price,
        min_price: null,
        track_stock: input.track_stock,
        allow_negative_stock: true,
        attributes: {},
        is_active: input.is_active,
        barcode_conflict: false,
        created_at: "",
        updated_at: "",
        version: 1,
        on_hand: 0,
      }),
    );
    vi.mocked(commands.updateProduct).mockImplementation((id, input) =>
      ok({
        id,
        kind: "standard",
        name: input.name,
        name_si: input.name_si ?? null,
        sku: input.sku.mode === "manual" ? input.sku.value : null,
        barcode: input.barcode.mode === "manual" ? input.barcode.value : null,
        category_id: input.category_id ?? null,
        default_cost: input.default_cost ?? null,
        price: input.price,
        min_price: null,
        track_stock: input.track_stock,
        allow_negative_stock: true,
        attributes: {},
        is_active: input.is_active,
        barcode_conflict: false,
        created_at: "",
        updated_at: "",
        version: 2,
        on_hand: 0,
      }),
    );
    vi.mocked(commands.adjustStock).mockImplementation(() => ok({} as never));
  });

  it("imports products from exported CSV, creates missing categories, maps cost and sets opening stock", async () => {
    renderWithProviders(
      <As perms={["PRODUCT_MANAGE", "VIEW_COST"]}>
        <ProductImportPanel />
      </As>,
    );

    const csvContent =
      "sku,barcode,name,name_si,category,price,default_cost,track_stock,on_hand,active\n" +
      "DSI,21000027,dsi,,DSI,3099.00,2603.00,yes,35,yes\n";

    const file = new File([csvContent], "products.csv", { type: "text/csv" });
    const fileInput = screen.getByLabelText(/CSV file/i);
    fireEvent.change(fileInput, { target: { files: [file] } });

    expect(await screen.findByRole("status")).toHaveTextContent("1 row(s) ready");

    const importBtn = screen.getByRole("button", { name: /Import 1 product/i });
    fireEvent.click(importBtn);

    await waitFor(() =>
      expect(commands.createCategory).toHaveBeenCalledWith({ name: "DSI", parent_id: null }),
    );

    await waitFor(() =>
      expect(commands.createProduct).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "dsi",
          price: 309_900,
          default_cost: 260_300,
          category_id: "cat-new",
          sku: { mode: "manual", value: "DSI" },
          barcode: { mode: "manual", value: "21000027" },
          track_stock: true,
          is_active: true,
        }),
      ),
    );

    await waitFor(() =>
      expect(commands.adjustStock).toHaveBeenCalledWith(
        {
          product_id: "prod-1",
          qty_delta: 35,
          note: expect.any(String),
        },
        null,
      ),
    );
  });

  it("updates existing products and adjusts stock delta when products already exist in database", async () => {
    const EXISTING_PRODUCT: Product = {
      id: "prod-existing",
      kind: "standard",
      name: "dsi",
      name_si: null,
      sku: "DSI",
      barcode: "21000027",
      category_id: null,
      default_cost: 0,
      price: 309_900,
      min_price: null,
      track_stock: true,
      allow_negative_stock: true,
      attributes: {},
      is_active: true,
      barcode_conflict: false,
      created_at: "",
      updated_at: "",
      version: 1,
      on_hand: 0,
    };
    vi.mocked(commands.listProducts).mockImplementation(() => ok([EXISTING_PRODUCT]));

    renderWithProviders(
      <As perms={["PRODUCT_MANAGE", "VIEW_COST"]}>
        <ProductImportPanel />
      </As>,
    );

    const csvContent =
      "sku,barcode,name,name_si,category,price,default_cost,track_stock,on_hand,active\n" +
      "DSI,21000027,dsi,,DSI,3099.00,2603.00,yes,35,yes\n";

    const file = new File([csvContent], "products.csv", { type: "text/csv" });
    const fileInput = screen.getByLabelText(/CSV file/i);
    fireEvent.change(fileInput, { target: { files: [file] } });

    expect(await screen.findByRole("status")).toHaveTextContent("1 row(s) ready");
    fireEvent.click(screen.getByRole("button", { name: /Import 1 product/i }));

    // Should call updateProduct rather than createProduct
    await waitFor(() =>
      expect(commands.updateProduct).toHaveBeenCalledWith(
        "prod-existing",
        expect.objectContaining({
          name: "dsi",
          price: 309_900,
          default_cost: 260_300,
          category_id: "cat-new",
        }),
      ),
    );

    // Delta = 35 - 0 = 35
    await waitFor(() =>
      expect(commands.adjustStock).toHaveBeenCalledWith(
        {
          product_id: "prod-existing",
          qty_delta: 35,
          note: expect.any(String),
        },
        null,
      ),
    );
  });

  it("imports multiple products with the same name if they have different SKUs or barcodes", async () => {
    let idCounter = 1;
    vi.mocked(commands.createProduct).mockImplementation((input) =>
      ok({
        id: `prod-${idCounter++}`,
        kind: "standard",
        name: input.name,
        name_si: input.name_si ?? null,
        sku: input.sku.mode === "manual" ? input.sku.value : null,
        barcode: input.barcode.mode === "manual" ? input.barcode.value : null,
        category_id: input.category_id ?? null,
        default_cost: input.default_cost ?? null,
        price: input.price,
        min_price: null,
        track_stock: input.track_stock,
        allow_negative_stock: true,
        attributes: {},
        is_active: input.is_active,
        barcode_conflict: false,
        created_at: "",
        updated_at: "",
        version: 1,
        on_hand: 0,
      }),
    );

    renderWithProviders(
      <As perms={["PRODUCT_MANAGE", "VIEW_COST"]}>
        <ProductImportPanel />
      </As>,
    );

    const csvContent =
      "sku,barcode,name,price\n" +
      "PEO525-S12,21000034,DSI,3099.00\n" +
      "PEO525-S1,21000041,DSI,3099.00\n";

    const file = new File([csvContent], "products.csv", { type: "text/csv" });
    const fileInput = screen.getByLabelText(/CSV file/i);
    fireEvent.change(fileInput, { target: { files: [file] } });

    expect(await screen.findByRole("status")).toHaveTextContent("2 row(s) ready");
    fireEvent.click(screen.getByRole("button", { name: /Import 2 product/i }));

    await waitFor(() => expect(commands.createProduct).toHaveBeenCalledTimes(2));
    expect(commands.updateProduct).not.toHaveBeenCalled();
    expect(await screen.findByText(/Imported 2 product\(s\), updated 0 product\(s\)/)).toBeInTheDocument();
  });
});
