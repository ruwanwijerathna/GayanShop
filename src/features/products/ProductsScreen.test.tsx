import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import {
  commands,
  type Category_Serialize as Category,
  type Perm,
  type Product_Serialize as Product,
} from "@/db/bindings";
import { chooseOption, err, ok, renderWithProviders, session } from "@/test/render";
import { PRODUCT_COLUMN_WIDTHS, ProductsScreen } from "./ProductsScreen";
import { matchesStockFilter, stockLevel } from "./stock";

vi.mock("@/db/bindings", () => ({
  commands: {
    listProducts: vi.fn(),
    listCategories: vi.fn(),
    createProduct: vi.fn(),
    updateProduct: vi.fn(),
    deleteProduct: vi.fn(),
    createCategory: vi.fn(),
    updateCategory: vi.fn(),
    deleteCategory: vi.fn(),
    adjustStock: vi.fn(),
  },
}));

const MANAGER: Perm[] = ["PRODUCT_MANAGE", "VIEW_COST"];

function product(over: Partial<Product>): Product {
  return {
    id: "p1",
    kind: "standard",
    name: "Blue Bucket 10L",
    name_si: "නිල් බාල්දිය",
    sku: "100001",
    barcode: "2100000000017",
    category_id: "c2",
    default_cost: 20_000,
    price: 45_000,
    min_price: null,
    track_stock: true,
    allow_negative_stock: true,
    attributes: {},
    is_active: true,
    barcode_conflict: false,
    updated_at: "2026-11-08T09:00:00.000Z",
    version: 1,
    on_hand: 45,
    ...over,
  };
}

const categories: Category[] = [
  { id: "c1", name: "Home", parent_id: null, product_count: 0 },
  { id: "c2", name: "Kitchen", parent_id: "c1", product_count: 1 },
];

const products: Product[] = [
  product({}),
  product({ id: "p2", name: "Black Sandal", name_si: null, sku: "100002", barcode: null, category_id: null }),
  product({ id: "p3", name: "Old Mug", name_si: null, sku: "100003", barcode: null, is_active: false }),
];

function As({ perms, children }: { perms: Perm[]; children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: perms })), [setSession, perms]);
  return <>{children}</>;
}

function renderAs(perms: Perm[]) {
  renderWithProviders(
    <As perms={perms}>
      <ProductsScreen />
    </As>,
  );
}

beforeAll(() => {
  // jsdom has no layout: give the virtualised table a viewport (see DataTable.test.tsx).
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 480 });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 900 });
});

beforeEach(() => {
  vi.mocked(commands.listProducts).mockImplementation(() => ok(products));
  vi.mocked(commands.listCategories).mockImplementation(() => ok(categories));
  vi.mocked(commands.createProduct).mockReset();
  vi.mocked(commands.updateProduct).mockReset();
  vi.mocked(commands.createCategory).mockReset();
  vi.mocked(commands.deleteCategory).mockReset();
  vi.mocked(commands.adjustStock).mockReset();
});

const grid = () => screen.getByRole("grid", { name: "Products" });

describe("ProductsScreen", () => {
  it("lists active products by their one name, with category path and cost, and filters as you type", async () => {
    renderAs(MANAGER);
    expect(await within(grid()).findByText("Blue Bucket 10L")).toBeInTheDocument();
    expect(within(grid()).queryByText("නිල් බාල්දිය")).not.toBeInTheDocument(); // no secondary name line
    expect(within(grid()).getByText("Home › Kitchen")).toBeInTheDocument();
    expect(within(grid()).getAllByText("Rs 200.00")).toHaveLength(2);
    expect(within(grid()).queryByText("Old Mug")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("2 products");

    fireEvent.change(screen.getByRole("searchbox", { name: "Search products" }), {
      target: { value: "බාල්" },
    });
    expect(within(grid()).queryByText("Black Sandal")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("1 product");

    fireEvent.change(screen.getByRole("searchbox", { name: "Search products" }), { target: { value: "" } });
    fireEvent.click(screen.getByRole("checkbox", { name: "Show inactive" }));
    expect(within(grid()).getByText("Old Mug")).toBeInTheDocument();
    expect(within(grid()).getByText("Inactive")).toBeInTheDocument();

    chooseOption(screen.getByRole("combobox", { name: "Filter by category" }), "Home › Kitchen");
    expect(screen.getByRole("status")).toHaveTextContent("2 products");
  });

  it("is read-only without PRODUCT_MANAGE and hides cost without VIEW_COST", async () => {
    renderAs(["SALE_CREATE"]);
    expect(await within(grid()).findByText("Blue Bucket 10L")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add product" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add category" })).not.toBeInTheDocument();
    expect(within(grid()).queryByText("Cost")).not.toBeInTheDocument();
    fireEvent.click(within(grid()).getByText("Blue Bucket 10L"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("creates a product with a generated SKU, sending integer cents", async () => {
    vi.mocked(commands.createProduct).mockImplementation(() => ok(product({ id: "new", name: "Soap" })));
    renderAs(MANAGER);
    fireEvent.click(await screen.findByRole("button", { name: "Add product" }));
    const dialog = await screen.findByRole("dialog");
    const name = within(dialog).getByLabelText("Product Name");
    expect(name).toHaveAttribute("placeholder", "Enter product name");
    expect(within(dialog).queryByLabelText(/Sinhala/)).not.toBeInTheDocument();
    expect(dialog.querySelectorAll("input[lang='si']")).toHaveLength(0);
    fireEvent.change(name, { target: { value: "Soap" } });
    fireEvent.change(within(dialog).getByLabelText("Selling price (Rs)"), { target: { value: "120.50" } });
    fireEvent.change(within(dialog).getByLabelText("Default cost (Rs)"), { target: { value: "80" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(commands.createProduct).toHaveBeenCalledWith(
      expect.objectContaining({
        name: "Soap",
        name_si: null,
        price: 12_050,
        default_cost: 8_000,
        sku: { mode: "generate" },
        barcode: { mode: "none" },
      }),
    );
  });

  it("creates a product with initial stock quantity and note", async () => {
    vi.mocked(commands.createProduct).mockImplementation(() =>
      ok(product({ id: "soap1", name: "Sunlight Soap" })),
    );
    vi.mocked(commands.adjustStock).mockImplementation(() => ok({} as never));
    renderAs(MANAGER);
    fireEvent.click(await screen.findByRole("button", { name: "Add product" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Product Name"), { target: { value: "Sunlight Soap" } });
    fireEvent.change(within(dialog).getByLabelText("Selling price (Rs)"), { target: { value: "150" } });
    fireEvent.change(within(dialog).getByLabelText("Stock Quantity"), { target: { value: "50" } });
    fireEvent.change(within(dialog).getByLabelText("Note"), { target: { value: "Initial opening batch" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(commands.createProduct).toHaveBeenCalled();
    expect(commands.adjustStock).toHaveBeenCalledWith(
      {
        product_id: "soap1",
        qty_delta: 50,
        note: "Initial opening batch",
      },
      null,
    );
  });

  it("keeps the dialog open with the field error for a bad amount, without calling Rust", async () => {
    renderAs(MANAGER);
    fireEvent.click(await screen.findByRole("button", { name: "Add product" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Selling price (Rs)"), { target: { value: "12.345" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByText("Enter an amount such as 450 or 450.50.")).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Selling price (Rs)")).toHaveAttribute("aria-invalid", "true");
    expect(commands.createProduct).not.toHaveBeenCalled();
  });

  it("edits a product from its row and shows Rust's validation message in the user's language", async () => {
    vi.mocked(commands.updateProduct).mockImplementation(() =>
      err({ kind: "VALIDATION", code: "BARCODE_TAKEN", detail: "4792024015103" }),
    );
    renderAs(MANAGER);
    fireEvent.click(await within(grid()).findByText("Blue Bucket 10L"));
    const dialog = await screen.findByRole("dialog", { name: "Edit product" });
    expect(within(dialog).getByLabelText("Selling price (Rs)")).toHaveValue("450.00");
    fireEvent.change(within(dialog).getByLabelText("Barcode"), { target: { value: "4792024015103" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "Another active product already has this barcode.",
    );
    expect(commands.updateProduct).toHaveBeenCalledWith(
      "p1",
      expect.objectContaining({
        barcode: { mode: "manual", value: "4792024015103" },
        sku: { mode: "manual", value: "100001" },
      }),
    );
  });

  it("adds a category and explains why one in use can't be deleted", async () => {
    vi.mocked(commands.createCategory).mockImplementation(() =>
      ok({ id: "c3", name: "Shoes", parent_id: null, product_count: 0 }),
    );
    vi.mocked(commands.deleteCategory).mockImplementation(() =>
      err({ kind: "VALIDATION", code: "CATEGORY_IN_USE", detail: "1 products" }),
    );
    renderAs(MANAGER);
    fireEvent.click(await screen.findByRole("button", { name: "Add category" }));
    const nameField = screen.getByLabelText("Category name");
    fireEvent.change(nameField, { target: { value: "Shoes" } });
    fireEvent.submit(nameField);
    await waitFor(() =>
      expect(commands.createCategory).toHaveBeenCalledWith({ name: "Shoes", parent_id: null }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Delete Home › Kitchen" }));
    expect(await screen.findByText("Move its products and sub-categories out first.")).toBeInTheDocument();
  });

  it("fits every column at the 1024×768 minimum window, cost included", () => {
    const total = Object.values(PRODUCT_COLUMN_WIDTHS).reduce((a, b) => a + b, 0);
    expect(total).toBeLessThanOrEqual(895);
  });

  it("shows live stock: a count, low, out, oversold, or a dash when untracked", async () => {
    vi.mocked(commands.listProducts).mockImplementation(() =>
      ok([
        product({ id: "a", name: "Plenty", on_hand: 45 }),
        product({ id: "b", name: "Few", sku: "100010", on_hand: 3 }),
        product({ id: "c", name: "None", sku: "100011", on_hand: 0 }),
        product({ id: "d", name: "Minus", sku: "100012", on_hand: -2 }),
        product({ id: "e", name: "Loose", sku: "100013", track_stock: false, on_hand: 0 }),
      ]),
    );
    renderAs(MANAGER);
    expect(await within(grid()).findByText("45 in stock")).toBeInTheDocument();
    expect(within(grid()).getByText("Stock")).toBeInTheDocument();
    expect(within(grid()).getByText("3 (Low stock)")).toBeInTheDocument();
    expect(within(grid()).getByText("0 (Out of stock)")).toBeInTheDocument();
    expect(within(grid()).getByText("-2 (Oversold)")).toBeInTheDocument();
    expect(within(grid()).getByText("Stock not tracked")).toBeInTheDocument();
  });

  it("filters to low stock (1–5) and out of stock (0 or oversold), and back to all", async () => {
    vi.mocked(commands.listProducts).mockImplementation(() =>
      ok([
        product({ id: "a", name: "Plenty", on_hand: 45 }),
        product({ id: "b", name: "Few", sku: "100010", on_hand: 5 }),
        product({ id: "c", name: "None", sku: "100011", on_hand: 0 }),
        product({ id: "d", name: "Minus", sku: "100012", on_hand: -2 }),
        product({ id: "e", name: "Loose", sku: "100013", track_stock: false, on_hand: 0 }),
      ]),
    );
    renderAs(MANAGER);
    await within(grid()).findByText("Plenty");
    const filters = screen.getByRole("group", { name: "Filter by stock" });
    expect(within(filters).getByRole("button", { name: "All Products" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    fireEvent.click(within(filters).getByRole("button", { name: "Low Stock (≤ 5)" }));
    expect(screen.getByRole("status")).toHaveTextContent("1 product");
    expect(within(grid()).getByText("Few")).toBeInTheDocument();

    fireEvent.click(within(filters).getByRole("button", { name: "Out of Stock (0)" }));
    expect(screen.getByRole("status")).toHaveTextContent("2 products");
    expect(within(grid()).getByText("None")).toBeInTheDocument();
    expect(within(grid()).getByText("Minus")).toBeInTheDocument();
    expect(within(grid()).queryByText("Loose")).not.toBeInTheDocument();

    fireEvent.click(within(filters).getByRole("button", { name: "All Products" }));
    expect(screen.getByRole("status")).toHaveTextContent("5 products");
  });

  it("classifies stock levels at the boundaries", () => {
    const at = (on_hand: number, track_stock = true) => ({ on_hand, track_stock });
    expect([6, 5, 1, 0, -1].map((n) => stockLevel(at(n)))).toEqual([
      "in_stock",
      "low",
      "low",
      "out",
      "oversold",
    ]);
    expect(stockLevel(at(-4, false))).toBe("untracked");
    expect(matchesStockFilter(at(0, false), "out")).toBe(false);
    expect(matchesStockFilter(at(-1), "out")).toBe(true);
    expect(matchesStockFilter(at(6), "low")).toBe(false);
  });
});
