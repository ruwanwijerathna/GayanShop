import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  commands,
  type LabelPrinterProfile_Serialize,
  type Product_Serialize as Product,
} from "@/db/bindings";
import { chooseOption, err, ok, renderWithProviders } from "@/test/render";
import { drawnJob, stickerLayout } from "@/test/labels";
import { AdjustStockDialog, RestockDialog } from "./ProductStockDialogs";

vi.mock("@/db/bindings", () => ({
  commands: {
    adjustStock: vi.fn(),
    recordDamage: vi.fn(),
    recordStockCount: vi.fn(),
    listLoginStaff: vi.fn(),
    requestApproval: vi.fn(),
    labelPrinterProfile: vi.fn(),
    printLabels: vi.fn(),
    labelPreview: vi.fn(),
  },
}));

vi.mock("@/lib/labelImage", () => ({
  labelBitmapFromElement: (_node: HTMLElement, width: number, height: number) =>
    Promise.resolve({ width, height, data: [] }),
}));

const LABEL_PRINTER: LabelPrinterProfile_Serialize = {
  transport: { kind: "spooler", printer_name: "Xprinter XP-365B" },
  language: "tspl",
  dpi: 203,
  darkness: 8,
  speed: 4,
  offset_x_mm: 0,
  offset_y_mm: 0,
  auto_print_on_receive: true,
  default_template_id: "three_up_30x16",
};

const POT = {
  id: "pot",
  kind: "standard",
  name: "Pot",
  name_si: null,
  sku: "100001",
  barcode: null,
  category_id: null,
  default_cost: 30_000,
  price: 50_000,
  min_price: null,
  track_stock: true,
  allow_negative_stock: true,
  attributes: {},
  is_active: true,
  on_hand: 10,
} as unknown as Product;

const CHANGE = { product_id: "pot", qty_delta: -2, cost_delta: -60_000, on_hand_after: 8 };

beforeEach(() => {
  vi.mocked(commands.adjustStock).mockReset();
  vi.mocked(commands.recordDamage).mockReset();
  vi.mocked(commands.recordStockCount).mockReset();
  vi.mocked(commands.listLoginStaff).mockImplementation(() =>
    ok([{ id: "sup", display_name: "Kamal", role_name: "Supervisor" }]),
  );
  vi.mocked(commands.requestApproval).mockImplementation(() => ok({ approval_id: "appr", expires_at: "" }));
  vi.mocked(commands.labelPrinterProfile).mockImplementation(() => ok(null));
  vi.mocked(commands.printLabels).mockReset();
  vi.mocked(commands.printLabels).mockImplementation(() => ok(null));
  vi.mocked(commands.labelPreview).mockImplementation((job) => ok(stickerLayout(job)));
});

describe("RestockDialog", () => {
  it("adds received units and previews the new on-hand", async () => {
    vi.mocked(commands.adjustStock).mockImplementation(() => ok({ ...CHANGE, qty_delta: 5 }));
    const onClose = vi.fn();
    renderWithProviders(<RestockDialog product={POT} onClose={onClose} />);
    const submit = await screen.findByRole("button", { name: "Add 0 to stock" });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Units received"), { target: { value: "5" } });
    expect(screen.getByTestId("on-hand-after")).toHaveTextContent("15");
    fireEvent.click(screen.getByRole("button", { name: "Add 5 to stock" }));
    await waitFor(() =>
      expect(commands.adjustStock).toHaveBeenCalledWith(
        { product_id: "pot", qty_delta: 5, note: "Restock" },
        null,
      ),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("prints one sticker per unit received on the saved sticker size when auto-print is on", async () => {
    vi.mocked(commands.labelPrinterProfile).mockImplementation(() => ok(LABEL_PRINTER));
    vi.mocked(commands.adjustStock).mockImplementation(() => ok({ ...CHANGE, qty_delta: 5 }));
    renderWithProviders(<RestockDialog product={POT} onClose={vi.fn()} />);
    await waitFor(() => expect(commands.labelPrinterProfile).toHaveBeenCalled());
    fireEvent.change(await screen.findByLabelText("Units received"), { target: { value: "5" } });
    fireEvent.click(screen.getByRole("button", { name: "Add 5 to stock" }));
    await waitFor(() =>
      expect(commands.printLabels).toHaveBeenCalledWith(
        drawnJob({
          template_id: "three_up_30x16",
          items: [{ product_id: "pot", copies: 5 }],
          fill_last_row: false,
        }),
        null,
      ),
    );
  });

  it("prints nothing when auto-print is off", async () => {
    vi.mocked(commands.labelPrinterProfile).mockImplementation(() =>
      ok({ ...LABEL_PRINTER, auto_print_on_receive: false }),
    );
    vi.mocked(commands.adjustStock).mockImplementation(() => ok({ ...CHANGE, qty_delta: 5 }));
    const onClose = vi.fn();
    renderWithProviders(<RestockDialog product={POT} onClose={onClose} />);
    await waitFor(() => expect(commands.labelPrinterProfile).toHaveBeenCalled());
    fireEvent.change(await screen.findByLabelText("Units received"), { target: { value: "5" } });
    fireEvent.click(screen.getByRole("button", { name: "Add 5 to stock" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(commands.printLabels).not.toHaveBeenCalled();
  });

  it("refuses zero, negative and non-numeric units", async () => {
    renderWithProviders(<RestockDialog product={POT} onClose={vi.fn()} />);
    const qty = await screen.findByLabelText("Units received");
    for (const bad of ["0", "-3", "1.5", "abc"]) {
      fireEvent.change(qty, { target: { value: bad } });
      expect(screen.getByRole("button", { name: /^Add \d+ to stock$/ })).toBeDisabled();
    }
    expect(commands.adjustStock).not.toHaveBeenCalled();
  });
});

describe("AdjustStockDialog", () => {
  it("saves a recount of the true shelf quantity", async () => {
    vi.mocked(commands.recordStockCount).mockImplementation(() => ok({ changes: [CHANGE], unchanged: 0 }));
    const onClose = vi.fn();
    renderWithProviders(<AdjustStockDialog product={POT} onClose={onClose} />);
    fireEvent.change(await screen.findByLabelText("Counted quantity"), { target: { value: "8" } });
    expect(screen.getByTestId("on-hand-after")).toHaveTextContent("8");
    fireEvent.click(screen.getByRole("button", { name: "Save count" }));
    await waitFor(() =>
      expect(commands.recordStockCount).toHaveBeenCalledWith(
        { lines: [{ product_id: "pot", counted: 8 }], note: null },
        null,
      ),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("writes off damage, asking a supervisor when needed", async () => {
    vi.mocked(commands.recordDamage)
      .mockImplementationOnce(() => err({ kind: "APPROVAL_REQUIRED", perm: "STOCK_DAMAGE" }))
      .mockImplementationOnce(() => ok(CHANGE));
    renderWithProviders(<AdjustStockDialog product={POT} onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: /Write off/ }));
    fireEvent.change(screen.getByLabelText("Units to write off"), { target: { value: "2" } });
    expect(screen.getByTestId("on-hand-after")).toHaveTextContent("8");
    chooseOption(screen.getByLabelText("Why"), "Expired");
    fireEvent.click(screen.getByRole("button", { name: "Write off" }));
    fireEvent.click(await screen.findByRole("button", { name: "Kamal" }));
    fireEvent.change(screen.getByLabelText("PIN"), { target: { value: "2222" } });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    await waitFor(() => expect(commands.recordDamage).toHaveBeenCalledTimes(2));
    expect(commands.recordDamage).toHaveBeenLastCalledWith(
      { product_id: "pot", qty: 2, reason: "expired", note: null },
      "appr",
    );
  });

  it("needs a reason note when the write-off reason is Other, and never accepts zero units", async () => {
    renderWithProviders(<AdjustStockDialog product={POT} onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: /Write off/ }));
    const submit = screen.getByRole("button", { name: "Write off" });
    fireEvent.change(screen.getByLabelText("Units to write off"), { target: { value: "0" } });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Units to write off"), { target: { value: "1" } });
    chooseOption(screen.getByLabelText("Why"), "Other");
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Reason (required)"), { target: { value: "spilled" } });
    expect(submit).toBeEnabled();
  });
});
