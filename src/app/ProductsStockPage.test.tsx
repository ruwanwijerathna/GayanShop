import { fireEvent, screen, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, type Category_Serialize as Category, type Perm } from "@/db/bindings";
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
  },
}));

const categories: Category[] = [
  { id: "c1", name: "Home", parent_id: null, product_count: 0 },
  { id: "c2", name: "Kitchen", parent_id: "c1", product_count: 0 },
];

function As({ perms, children }: { perms: Perm[]; children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: perms })), [setSession, perms]);
  return <>{children}</>;
}

beforeEach(() => {
  vi.mocked(commands.listProducts).mockImplementation(() => ok([]));
  vi.mocked(commands.listCategories).mockImplementation(() => ok(categories));
  vi.mocked(commands.appStatus).mockImplementation(() => ok({ device_code: "T02" } as never));
  vi.mocked(commands.labelPrinterProfile).mockImplementation(() => ok(null));
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
});
