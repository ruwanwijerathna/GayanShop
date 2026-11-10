import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  commands,
  type LabelPreview_Serialize as LabelPreview,
  type LabelTemplate_Serialize as LabelTemplate,
} from "@/db/bindings";
import { err, ok, renderWithProviders } from "@/test/render";
import { PrintLabelsDialog } from "./PrintLabelsDialog";

vi.mock("@/db/bindings", () => ({
  commands: {
    labelTemplates: vi.fn(),
    labelPreview: vi.fn(),
    printLabels: vi.fn(),
    listLoginStaff: vi.fn(),
    requestApproval: vi.fn(),
  },
}));

const TEMPLATES = [
  { id: "two_up_2x1", name: "2-up", columns: 2, dpi: 203 },
  { id: "three_up_1x1", name: "3-up", columns: 3, dpi: 203 },
] as unknown as LabelTemplate[];

const PREVIEW: LabelPreview = {
  template_id: "two_up_2x1",
  dpi: 203,
  columns: 2,
  rows: 1,
  roll_width_mm: 106.6,
  label_w_mm: 50.8,
  label_h_mm: 25.4,
  row_gap_mm: 2,
  labels: [
    {
      row: 0,
      col: 0,
      product_id: "p1",
      x_mm: 1.5,
      texts: [
        {
          kind: "price",
          x_mm: 31.5,
          y_mm: 10.5,
          w_mm: 18.8,
          h_mm: 4,
          font_size: 3,
          bold: true,
          align: "right",
          lines: ["Rs. 1,250"],
        },
      ],
      barcode: {
        symbology: "ean13",
        data: "2000123456782",
        x_mm: 5,
        y_mm: 15,
        bar_h_mm: 6.5,
        module_dots: 3,
        module_mm: 0.375,
        widths: [1, 1, 1, 3, 2, 1, 1],
        fits: true,
      },
    },
  ],
  warnings: [{ code: "BARCODE_TOO_LONG", product_id: "p1" }],
};

beforeEach(() => {
  localStorage.clear();
  vi.mocked(commands.labelTemplates).mockImplementation(() => ok(TEMPLATES));
  vi.mocked(commands.labelPreview).mockImplementation(() => ok(PREVIEW));
  vi.mocked(commands.printLabels).mockReset();
  vi.mocked(commands.listLoginStaff).mockImplementation(() =>
    ok([
      { id: "cashier", display_name: "Nimal", role_name: "Cashier" },
      { id: "sup", display_name: "Kamal", role_name: "Supervisor" },
    ]),
  );
  vi.mocked(commands.requestApproval).mockImplementation(() => ok({ approval_id: "appr", expires_at: "" }));
});

describe("PrintLabelsDialog", () => {
  it("previews exactly what will print, with warnings, then prints", async () => {
    vi.mocked(commands.printLabels).mockImplementation(() => ok(PREVIEW));
    const onClose = vi.fn();
    renderWithProviders(
      <PrintLabelsDialog
        items={[{ product_id: "p1", name: "Ladies Sandal", copies: 12 }]}
        onClose={onClose}
      />,
    );
    const svg = await screen.findByRole("img", { name: "Label preview" });
    expect(within(svg).getByText("Rs. 1,250")).toBeInTheDocument();
    expect(within(svg).getByTestId("barcode")).toHaveAttribute("data-fits", "true");
    expect(screen.getByText(/Ladies Sandal: the code is too long/)).toBeInTheDocument();
    expect(commands.labelPreview).toHaveBeenCalledWith({
      template_id: "two_up_2x1",
      items: [{ product_id: "p1", copies: 12 }],
      fill_last_row: false,
    });

    fireEvent.change(screen.getByLabelText("Copies of Ladies Sandal"), { target: { value: "3" } });
    fireEvent.change(screen.getByLabelText("Label roll"), { target: { value: "three_up_1x1" } });
    fireEvent.click(screen.getByRole("button", { name: "Print 3 label(s)" }));
    await waitFor(() =>
      expect(commands.printLabels).toHaveBeenCalledWith(
        { template_id: "three_up_1x1", items: [{ product_id: "p1", copies: 3 }], fill_last_row: false },
        null,
      ),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(localStorage.getItem("gayanshop.label.template")).toBe("three_up_1x1");
  });

  it("refuses zero copies", async () => {
    renderWithProviders(
      <PrintLabelsDialog
        items={[{ product_id: "p1", name: "Ladies Sandal", copies: 1 }]}
        onClose={vi.fn()}
      />,
    );
    await screen.findByRole("img", { name: "Label preview" });
    fireEvent.change(screen.getByLabelText("Copies of Ladies Sandal"), { target: { value: "0" } });
    expect(screen.getByRole("button", { name: /Print 0 label/ })).toBeDisabled();
  });

  it("asks a supervisor when the cashier lacks LABEL_PRINT, then prints with the approval", async () => {
    vi.mocked(commands.printLabels)
      .mockImplementationOnce(() => err({ kind: "APPROVAL_REQUIRED", perm: "LABEL_PRINT" }))
      .mockImplementationOnce(() => ok(PREVIEW));
    renderWithProviders(
      <PrintLabelsDialog
        items={[{ product_id: "p1", name: "Ladies Sandal", copies: 2 }]}
        onClose={vi.fn()}
      />,
    );
    await screen.findByRole("img", { name: "Label preview" });
    fireEvent.click(screen.getByRole("button", { name: "Print 2 label(s)" }));
    fireEvent.click(await screen.findByRole("button", { name: "Kamal" }));
    fireEvent.change(screen.getByLabelText("PIN"), { target: { value: "2222" } });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    await waitFor(() => expect(commands.printLabels).toHaveBeenCalledTimes(2));
    expect(commands.requestApproval).toHaveBeenCalledWith("LABEL_PRINT", "labels", "sup", "2222");
    expect(vi.mocked(commands.printLabels).mock.calls[1]?.[1]).toBe("appr");
  });
});
