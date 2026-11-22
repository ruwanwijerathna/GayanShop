import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, type LabelTemplate_Serialize as LabelTemplate } from "@/db/bindings";
import { drawnJob, stickerLayout } from "@/test/labels";
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

vi.mock("@/lib/labelImage", () => ({
  labelBitmapFromElement: (_node: HTMLElement, width: number, height: number) =>
    Promise.resolve({ width, height, data: [] }),
}));

const TEMPLATES = [
  { id: "two_up_50x25", name: "2-up", columns: 2, dpi: 203 },
  { id: "three_up_30x16", name: "3-up", columns: 3, dpi: 203 },
] as unknown as LabelTemplate[];

beforeEach(() => {
  localStorage.clear();
  vi.mocked(commands.labelTemplates).mockImplementation(() => ok(TEMPLATES));
  vi.mocked(commands.labelPreview).mockImplementation((job) =>
    ok(stickerLayout(job, [{ code: "BARCODE_TOO_LONG", product_id: "p1" }])),
  );
  vi.mocked(commands.printLabels).mockReset();
  vi.mocked(commands.printLabels).mockImplementation(() => ok(null));
  vi.mocked(commands.listLoginStaff).mockImplementation(() =>
    ok([
      { id: "cashier", display_name: "Nimal", role_name: "Cashier" },
      { id: "sup", display_name: "Kamal", role_name: "Supervisor" },
    ]),
  );
  vi.mocked(commands.requestApproval).mockImplementation(() => ok({ approval_id: "appr", expires_at: "" }));
});

describe("PrintLabelsDialog", () => {
  it("previews a single label, with warnings, then prints every copy as the drawn sticker", async () => {
    const onClose = vi.fn();
    renderWithProviders(
      <PrintLabelsDialog
        items={[{ product_id: "p1", name: "Ladies Sandal", copies: 12 }]}
        onClose={onClose}
      />,
    );
    const preview = await screen.findByRole("img", { name: "Label preview" });
    expect(within(preview).getByText("Rs. 1,250")).toBeInTheDocument();
    expect(within(preview).getByText("21000423")).toBeInTheDocument();
    const bars = within(preview).getByTestId("sticker-bars");
    expect(bars).toHaveAttribute("shape-rendering", "crispEdges");
    // The bars are whole-dot rectangles: modules [2, 1, 1, 2, 2, 2] at 4 dots each, so bars of 8, 4 and 8
    // dots at dot offsets 0, 12 and 24.
    expect(
      [...bars.querySelectorAll("rect")].map((r) => [r.getAttribute("x"), r.getAttribute("width")]),
    ).toEqual([
      ["0", "8"],
      ["12", "4"],
      ["24", "8"],
    ]);
    expect(screen.getByText(/Ladies Sandal: the code is too long/)).toBeInTheDocument();
    expect(commands.labelPreview).toHaveBeenCalledWith({
      template_id: "two_up_50x25",
      items: [{ product_id: "p1", copies: 1 }],
      fill_last_row: false,
    });
    expect(screen.getAllByRole("img", { name: "Label preview" })).toHaveLength(1);

    fireEvent.change(screen.getByLabelText("Copies of Ladies Sandal"), { target: { value: "3" } });
    fireEvent.click(screen.getByRole("radio", { name: "30 x 16 mm, 3 across" }));
    fireEvent.click(screen.getByRole("button", { name: "Print 3 label(s)" }));
    await waitFor(() =>
      expect(commands.printLabels).toHaveBeenCalledWith(
        drawnJob({
          template_id: "three_up_30x16",
          items: [{ product_id: "p1", copies: 3 }],
          fill_last_row: false,
        }),
        null,
      ),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(localStorage.getItem("gayanshop.label.template")).toBe("three_up_30x16");
  });

  it("previews one label however many copies are requested", async () => {
    renderWithProviders(
      <PrintLabelsDialog
        items={[{ product_id: "p1", name: "Ladies Sandal", copies: 20 }]}
        onClose={vi.fn()}
      />,
    );
    await screen.findByRole("img", { name: "Label preview" });
    expect(screen.getAllByRole("img", { name: "Label preview" })).toHaveLength(1);
    for (const [job] of vi.mocked(commands.labelPreview).mock.calls) {
      expect(job.items).toEqual([{ product_id: "p1", copies: 1 }]);
    }
    fireEvent.click(screen.getByRole("button", { name: "More copies of Ladies Sandal" }));
    fireEvent.click(screen.getByRole("button", { name: "Print 21 label(s)" }));
    await waitFor(() =>
      expect(commands.printLabels).toHaveBeenCalledWith(
        drawnJob({
          template_id: "two_up_50x25",
          items: [{ product_id: "p1", copies: 21 }],
          fill_last_row: false,
        }),
        null,
      ),
    );
  });

  it("lets several products be previewed one at a time, and checks warnings across all", async () => {
    renderWithProviders(
      <PrintLabelsDialog
        items={[
          { product_id: "p1", name: "Ladies Sandal", copies: 4 },
          { product_id: "p2", name: "Kids Shoe", copies: 6 },
        ]}
        onClose={vi.fn()}
      />,
    );
    await screen.findByRole("img", { name: "Label preview" });
    fireEvent.click(screen.getByRole("button", { name: "Kids Shoe" }));
    await waitFor(() =>
      expect(commands.labelPreview).toHaveBeenCalledWith({
        template_id: "two_up_50x25",
        items: [{ product_id: "p2", copies: 1 }],
        fill_last_row: false,
      }),
    );
    expect(commands.labelPreview).toHaveBeenCalledWith({
      template_id: "two_up_50x25",
      items: [
        { product_id: "p1", copies: 4 },
        { product_id: "p2", copies: 6 },
      ],
      fill_last_row: false,
    });
    expect(await screen.findAllByRole("img", { name: "Label preview" })).toHaveLength(1);
  });

  it("offers to fill the last row only on rolls with several labels across", async () => {
    vi.mocked(commands.labelTemplates).mockImplementation(() =>
      ok([{ id: "one_up", name: "1-up", columns: 1, dpi: 203 }] as unknown as LabelTemplate[]),
    );
    localStorage.setItem("gayanshop.label.template", "one_up");
    renderWithProviders(
      <PrintLabelsDialog
        items={[{ product_id: "p1", name: "Ladies Sandal", copies: 2 }]}
        onClose={vi.fn()}
      />,
    );
    await screen.findByRole("img", { name: "Label preview" });
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
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
      .mockImplementationOnce(() => ok(null));
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

  it("shows Rust's refusal of a picture and keeps the dialog open", async () => {
    vi.mocked(commands.printLabels).mockImplementation(() =>
      err({
        kind: "VALIDATION",
        code: "LABEL_IMAGE_INVALID",
        detail: "240x128 dots, the sticker is 400x200",
      }),
    );
    const onClose = vi.fn();
    renderWithProviders(
      <PrintLabelsDialog
        items={[{ product_id: "p1", name: "Ladies Sandal", copies: 1 }]}
        onClose={onClose}
      />,
    );
    await screen.findByRole("img", { name: "Label preview" });
    fireEvent.click(screen.getByRole("button", { name: "Print 1 label(s)" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/sticker/i);
    expect(onClose).not.toHaveBeenCalled();
  });
});
