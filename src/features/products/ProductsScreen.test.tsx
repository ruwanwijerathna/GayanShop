import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import {
  commands,
  type Category_Serialize as Category,
  type Perm,
  type Product_Serialize as Product,
} from "@/db/bindings";
import { err, ok, renderWithProviders, session } from "@/test/render";
import { ProductsScreen } from "./ProductsScreen";
import { stockLevel } from "./stock";

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
    appStatus: vi.fn(),
    adjustStock: vi.fn(),
    stockMovements: vi.fn(),
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
  return renderWithProviders(
    <As perms={perms}>
      <ProductsScreen />
    </As>,
  );
}

beforeEach(() => {
  vi.mocked(commands.listProducts).mockImplementation(() => ok(products));
  vi.mocked(commands.listCategories).mockImplementation(() => ok(categories));
  vi.mocked(commands.createProduct).mockReset();
  vi.mocked(commands.updateProduct).mockReset();
  vi.mocked(commands.createCategory).mockReset();
  vi.mocked(commands.appStatus).mockImplementation(() => ok({ device_code: "T02" } as never));
  vi.mocked(commands.deleteCategory).mockReset();
  vi.mocked(commands.adjustStock).mockReset();
});

const grid = () => screen.getByRole("grid", { name: "Products", hidden: true });

/** Radix menus open on Enter / pointer-down; jsdom gets the keyboard route. */
const openMenu = (trigger: HTMLElement) => fireEvent.keyDown(trigger, { key: "Enter" });
/** A toolbar filter button (the column header may share its name, so look outside the table). */
const facet = (name: string) =>
  screen.getAllByRole("button", { name }).find((b) => !grid().contains(b)) as HTMLElement;
const pickFacet = (filter: string, option: RegExp | string) => {
  openMenu(facet(filter));
  fireEvent.click(screen.getByRole("menuitemcheckbox", { name: option }));
  fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
};
const openCategories = async () => {
  fireEvent.click(await screen.findByRole("button", { name: /^Categories/ }));
  return screen.findByRole("dialog", { name: "Categories" });
};

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
    pickFacet("Status", /^Inactive/);
    expect(within(grid()).getByText("Old Mug")).toBeInTheDocument();
    expect(within(grid()).getByText("Inactive")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("3 products");

    pickFacet("Category", /Home › Kitchen/);
    expect(screen.getByRole("status")).toHaveTextContent("2 products");
    expect(within(grid()).queryByText("Black Sandal")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Reset" }));
    expect(screen.getByRole("status")).toHaveTextContent("3 products");
  });

  it("is read-only without PRODUCT_MANAGE and hides cost without VIEW_COST", async () => {
    renderAs(["SALE_CREATE"]);
    expect(await within(grid()).findByText("Blue Bucket 10L")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add product" })).not.toBeInTheDocument();
    expect(within(grid()).queryByText("Cost")).not.toBeInTheDocument();
    fireEvent.click(within(grid()).getByText("Blue Bucket 10L"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    const categories = await openCategories();
    expect(within(categories).getByText("Kitchen")).toBeInTheDocument();
    expect(within(categories).queryByRole("button", { name: "Add category" })).not.toBeInTheDocument();
    expect(within(categories).queryByRole("button", { name: /^Delete / })).not.toBeInTheDocument();
  });

  it("fills the SKU and barcode inputs instantly on Generate, sending them as typed codes", async () => {
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
    fireEvent.change(within(dialog).getByLabelText("Purchase price (Rs)"), { target: { value: "80" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Generate SKU" }));
    expect(within(dialog).getByLabelText(/^SKU/)).toHaveValue("200001");
    fireEvent.click(within(dialog).getByRole("button", { name: "Generate barcode" }));
    expect(within(dialog).getByLabelText(/^Barcode/)).toHaveValue("2200000000019");
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(commands.createProduct).toHaveBeenCalledWith(
      expect.objectContaining({
        name: "Soap",
        name_si: null,
        price: 12_050,
        default_cost: 8_000,
        sku: { mode: "manual", value: "200001" },
        barcode: { mode: "manual", value: "2200000000019" },
      }),
    );
  });

  it("generates a different code each time Generate is clicked", async () => {
    renderAs(MANAGER);
    fireEvent.click(await screen.findByRole("button", { name: "Add product" }));
    const dialog = await screen.findByRole("dialog");
    const generate = within(dialog).getByRole("button", { name: "Generate SKU" });
    fireEvent.click(generate);
    fireEvent.click(generate);
    expect(within(dialog).getByLabelText(/^SKU/)).toHaveValue("200002");
  });

  it("puts the note last, as a multi-line textarea", async () => {
    renderAs(MANAGER);
    fireEvent.click(await screen.findByRole("button", { name: "Add product" }));
    const dialog = await screen.findByRole("dialog");
    const note = within(dialog).getByLabelText("Note");
    expect(note.tagName).toBe("TEXTAREA");
    const fields = Array.from(dialog.querySelectorAll("input, textarea, [role='switch']"));
    expect(fields.indexOf(note)).toBe(fields.length - 1 - fields.slice(fields.indexOf(note) + 1).length);
    expect(fields.slice(fields.indexOf(note) + 1)).toHaveLength(0);
  });

  it("suggests existing categories while typing and creates a new one on the fly", async () => {
    vi.mocked(commands.createCategory).mockImplementation(() =>
      ok({ id: "c9", name: "Shoes", parent_id: null } as never),
    );
    vi.mocked(commands.createProduct).mockImplementation(() => ok(product({ id: "n", name: "Boot" })));
    renderAs(MANAGER);
    fireEvent.click(await screen.findByRole("button", { name: "Add product" }));
    const dialog = await screen.findByRole("dialog");
    const box = within(dialog).getByRole("combobox", { name: "Category" });
    fireEvent.change(box, { target: { value: "kitch" } });
    expect(await screen.findByRole("option", { name: "Home › Kitchen" })).toBeInTheDocument();
    fireEvent.change(box, { target: { value: "Shoes" } });
    expect(screen.queryByRole("option", { name: "Home › Kitchen" })).not.toBeInTheDocument();
    fireEvent.mouseDown(await screen.findByRole("option", { name: "Create “Shoes”" }));
    await waitFor(() =>
      expect(commands.createCategory).toHaveBeenCalledWith({ name: "Shoes", parent_id: null }),
    );
    await waitFor(() => expect(box).toHaveValue("Shoes"));
    fireEvent.change(within(dialog).getByLabelText("Product Name"), { target: { value: "Boot" } });
    fireEvent.change(within(dialog).getByLabelText("Selling price (Rs)"), { target: { value: "900" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(commands.createProduct).toHaveBeenCalledWith(expect.objectContaining({ category_id: "c9" })),
    );
  });

  it("picks an existing category with the keyboard and offers no duplicate create row", async () => {
    vi.mocked(commands.createProduct).mockImplementation(() => ok(product({ id: "k", name: "Pan" })));
    renderAs(MANAGER);
    fireEvent.click(await screen.findByRole("button", { name: "Add product" }));
    const dialog = await screen.findByRole("dialog");
    const box = within(dialog).getByRole("combobox", { name: "Category" });
    fireEvent.change(box, { target: { value: "Home › Kitchen" } });
    expect(screen.queryByRole("option", { name: /^Create/ })).not.toBeInTheDocument();
    fireEvent.keyDown(box, { key: "Enter" });
    expect(box).toHaveValue("Home › Kitchen");
    fireEvent.change(within(dialog).getByLabelText("Product Name"), { target: { value: "Pan" } });
    fireEvent.change(within(dialog).getByLabelText("Selling price (Rs)"), { target: { value: "100" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(commands.createProduct).toHaveBeenCalled());
    expect(commands.createCategory).not.toHaveBeenCalled();
  });

  it("creates a product with no SKU or barcode when left blank (None state)", async () => {
    vi.mocked(commands.createProduct).mockImplementation(() => ok(product({ id: "new2", name: "Apple" })));
    renderAs(MANAGER);
    fireEvent.click(await screen.findByRole("button", { name: "Add product" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Product Name"), { target: { value: "Apple" } });
    fireEvent.change(within(dialog).getByLabelText("Selling price (Rs)"), { target: { value: "50" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(commands.createProduct).toHaveBeenCalledWith(
      expect.objectContaining({
        name: "Apple",
        sku: { mode: "none" },
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
    fireEvent.change(within(dialog).getByLabelText("Quantity"), { target: { value: "50" } });
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
    fireEvent.change(within(dialog).getByLabelText(/^Barcode/), { target: { value: "4792024015103" } });
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
    const dialog = await openCategories();
    fireEvent.click(within(dialog).getByRole("button", { name: "Add category" }));
    const nameField = within(dialog).getByLabelText("Category name");
    fireEvent.change(nameField, { target: { value: "Shoes" } });
    fireEvent.submit(nameField);
    await waitFor(() =>
      expect(commands.createCategory).toHaveBeenCalledWith({ name: "Shoes", parent_id: null }),
    );

    fireEvent.click(within(dialog).getByRole("button", { name: "Delete Home › Kitchen" }));
    expect(within(dialog).getByText("Delete “Kitchen”?")).toBeInTheDocument();
    expect(commands.deleteCategory).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(
      await within(dialog).findByText("Move its products and sub-categories out first."),
    ).toBeInTheDocument();
    expect(commands.deleteCategory).toHaveBeenCalledWith("c2");
  });

  it("shows a category's products from the category manager", async () => {
    renderAs(MANAGER);
    const dialog = await openCategories();
    fireEvent.click(within(dialog).getByRole("button", { name: "Show products in Home › Kitchen" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByRole("status")).toHaveTextContent("1 product");
    expect(within(grid()).queryByText("Black Sandal")).not.toBeInTheDocument();
  });

  it("hides and shows columns from the View menu", async () => {
    renderAs(MANAGER);
    await within(grid()).findByText("Blue Bucket 10L");
    expect(within(grid()).getByText("Cost")).toBeInTheDocument();
    openMenu(screen.getByRole("button", { name: "View" }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Cost" }));
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(within(grid()).queryByText("Cost")).not.toBeInTheDocument();
    expect(within(grid()).getByText("Price")).toBeInTheDocument();
  });

  it("sorts from the column menu", async () => {
    renderAs(MANAGER);
    await within(grid()).findByText("Blue Bucket 10L");
    const names = () =>
      within(grid())
        .getAllByRole("row")
        .slice(1)
        .map((r) => r.querySelectorAll("td")[1]?.textContent ?? "");
    expect(names()[0]).toContain("Black Sandal");
    openMenu(within(grid()).getByRole("button", { name: "Name" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Desc" }));
    expect(names()[0]).toContain("Blue Bucket 10L");
  });

  it("pages through a long list", async () => {
    vi.mocked(commands.listProducts).mockImplementation(() =>
      ok(
        Array.from({ length: 30 }, (_, i) =>
          product({
            id: `p${i}`,
            name: `Item ${String(i).padStart(2, "0")}`,
            sku: String(200000 + i),
            barcode: null,
          }),
        ),
      ),
    );
    renderAs(MANAGER);
    await within(grid()).findByText("Item 00");
    expect(within(grid()).queryByText("Item 25")).not.toBeInTheDocument();
    expect(screen.getByText("Page 1 of 2")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    expect(await within(grid()).findByText("Item 25")).toBeInTheDocument();
    expect(screen.getByText("Page 2 of 2")).toBeInTheDocument();
  });

  it("offers movement history in the row menu only with REPORT_FINANCIAL", async () => {
    vi.mocked(commands.stockMovements).mockImplementation(() => ok({ rows: [], next_cursor: null }));
    const { unmount } = renderAs(MANAGER);
    await within(grid()).findByText("Blue Bucket 10L");
    openMenu(screen.getByRole("button", { name: "Actions for Blue Bucket 10L" }));
    expect(screen.queryByRole("menuitem", { name: "Movement history" })).not.toBeInTheDocument();
    unmount();

    renderAs([...MANAGER, "REPORT_FINANCIAL"]);
    await within(grid()).findByText("Blue Bucket 10L");
    openMenu(screen.getByRole("button", { name: "Actions for Blue Bucket 10L" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Movement history" }));
    const sheet = await screen.findByRole("dialog");
    expect(within(sheet).getByText("Blue Bucket 10L")).toBeInTheDocument();
    expect(commands.stockMovements).toHaveBeenCalledWith("p1", null, null, 100);
  });

  it("opens the editor from the row actions menu", async () => {
    renderAs(MANAGER);
    await within(grid()).findByText("Blue Bucket 10L");
    openMenu(screen.getByRole("button", { name: "Actions for Blue Bucket 10L" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit product" }));
    expect(await screen.findByRole("dialog", { name: "Edit product" })).toBeInTheDocument();
  });

  it("offers Restock and Adjust stock on a tracked product's row, opening separate dialogs", async () => {
    renderAs(["SALE_CREATE"]);
    await within(grid()).findByText("Blue Bucket 10L");
    openMenu(screen.getByRole("button", { name: "Actions for Blue Bucket 10L" }));
    expect(screen.getByRole("menuitem", { name: "Restock" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Adjust stock" }));
    expect(await screen.findByRole("dialog", { name: "Adjust stock: Blue Bucket 10L" })).toBeInTheDocument();
    expect(screen.getByLabelText("Counted quantity")).toBeInTheDocument();
  });

  it("keeps stock out of the product editor: it points to the row actions instead", async () => {
    renderAs(MANAGER);
    await within(grid()).findByText("Blue Bucket 10L");
    openMenu(screen.getByRole("button", { name: "Actions for Blue Bucket 10L" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit product" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).queryByLabelText("Quantity")).not.toBeInTheDocument();
    expect(within(dialog).getByText(/use Restock or Adjust stock/)).toBeInTheDocument();
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

  it("filters by stock level, with live counts on each option", async () => {
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
    expect(screen.getByRole("status")).toHaveTextContent("5 products");

    openMenu(facet("Stock level"));
    expect(screen.getByRole("menuitemcheckbox", { name: /Low stock/ })).toHaveTextContent("1");
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: /Low stock/ }));
    expect(screen.getByRole("status", { hidden: true })).toHaveTextContent("1 product");
    expect(within(grid()).getByText("Few")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: /Out of stock/ }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: /Oversold/ }));
    expect(screen.getByRole("status", { hidden: true })).toHaveTextContent("3 products");
    expect(within(grid()).getByText("None")).toBeInTheDocument();
    expect(within(grid()).getByText("Minus")).toBeInTheDocument();
    expect(within(grid()).queryByText("Loose")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("menuitem", { name: "Clear filter" }));
    expect(screen.getByRole("status", { hidden: true })).toHaveTextContent("5 products");
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
  });
});
