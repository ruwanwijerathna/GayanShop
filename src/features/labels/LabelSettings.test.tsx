import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import {
  commands,
  type LabelPrinterProfile_Serialize,
  type LabelTemplate_Serialize as LabelTemplate,
  type Perm,
} from "@/db/bindings";
import { chooseOption, err, ok, renderWithProviders, session } from "@/test/render";
import { STICKER, stickerLayout } from "@/test/labels";
import { LabelSettings, parseMm, toForm, toProfile } from "./LabelSettings";

vi.mock("@/db/bindings", () => ({
  commands: {
    labelPrinterProfile: vi.fn(),
    labelTemplates: vi.fn(),
    setLabelPrinterProfile: vi.fn(),
    calibrateLabelTemplate: vi.fn(),
    labelAlignmentTest: vi.fn(),
    labelSamplePreview: vi.fn(),
    labelSamplePrint: vi.fn(),
    listSystemPrinters: vi.fn(),
  },
}));

vi.mock("@/lib/labelImage", () => ({
  labelBitmapFromElement: (_node: HTMLElement, width: number, height: number) =>
    Promise.resolve({ width, height, data: [] }),
}));

const TEMPLATE = {
  id: "three_up_30x16",
  name: "3-up",
  columns: 3,
  label_w_mm: 30,
  label_h_mm: 16,
  col_gap_mm: 2,
  row_gap_mm: 2,
  left_margin_mm: 1.5,
  top_margin_mm: 0,
  dpi: 203,
  language: "tspl",
  symbology: "code128",
  elements: [],
} as unknown as LabelTemplate;

const TWO_UP = {
  ...TEMPLATE,
  id: "two_up_50x25",
  name: "2-up",
  columns: 2,
  col_gap_mm: 3,
  top_margin_mm: 0.5,
} as unknown as LabelTemplate;

const THREE_UP_20 = { ...TEMPLATE, id: "three_up_30x20", name: "3-up 20" } as unknown as LabelTemplate;

const PROFILE: LabelPrinterProfile_Serialize = {
  transport: { kind: "network", host: "192.168.1.60", port: 9100 },
  language: "zpl",
  dpi: 300,
  darkness: 10,
  speed: 3,
  offset_x_mm: 0.5,
  offset_y_mm: -0.25,
  auto_print_on_receive: true,
  default_template_id: "three_up_30x16",
};

const SAMPLE = stickerLayout({
  template_id: "three_up_30x16",
  items: [{ product_id: "sample", copies: 1 }],
  fill_last_row: false,
});

const MANAGER: Perm[] = ["SETTINGS_MANAGE"];

function As({ children }: { children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: MANAGER })), [setSession]);
  return <>{children}</>;
}

beforeEach(() => {
  // Call counts start fresh for every test (the implementations are set again below).
  vi.clearAllMocks();
  vi.mocked(commands.labelPrinterProfile).mockImplementation(() => ok(null));
  vi.mocked(commands.labelTemplates).mockImplementation(() => ok([TEMPLATE]));
  vi.mocked(commands.setLabelPrinterProfile).mockImplementation(() => ok(null));
  vi.mocked(commands.calibrateLabelTemplate).mockImplementation(() => ok(TEMPLATE));
  vi.mocked(commands.labelAlignmentTest).mockImplementation(() => ok(null));
  vi.mocked(commands.labelSamplePrint).mockImplementation(() => ok(null));
  vi.mocked(commands.labelSamplePreview).mockImplementation(() => ok(SAMPLE));
  vi.mocked(commands.listSystemPrinters).mockImplementation(() =>
    ok(["Xprinter XP-365B", "Microsoft Print to PDF"]),
  );
});

describe("label settings mapping", () => {
  it("round-trips the profile and parses millimetres", () => {
    expect(toProfile(toForm(PROFILE))).toEqual(PROFILE);
    expect(parseMm("1.5")).toBe(1.5);
    expect(parseMm("-0.25")).toBe(-0.25);
    expect(parseMm("abc")).toBeNull();
    expect(toProfile({ ...toForm(PROFILE), darkness: "x" })).toBeNull();
  });
});

describe("LabelSettings", () => {
  beforeEach(() => localStorage.clear());

  it("saves a TSPL USB label printer", async () => {
    renderWithProviders(
      <As>
        <LabelSettings />
      </As>,
    );
    const card = await screen.findByRole("region", { name: "Label printer" });
    chooseOption(await within(card).findByLabelText("Printer"), "Xprinter XP-365B");
    fireEvent.click(within(card).getByRole("button", { name: "Save label printer" }));
    await waitFor(() =>
      expect(commands.setLabelPrinterProfile).toHaveBeenCalledWith({
        transport: { kind: "spooler", printer_name: "Xprinter XP-365B" },
        language: "tspl",
        dpi: 203,
        darkness: 8,
        speed: 4,
        offset_x_mm: 0,
        offset_y_mm: 0,
        auto_print_on_receive: false,
        default_template_id: "three_up_30x16",
      }),
    );
    // Nothing in the calibration was changed, so the roll is not rewritten.
    expect(commands.calibrateLabelTemplate).not.toHaveBeenCalled();
  });

  it("saves the print-when-stock-arrives switch with the printer", async () => {
    renderWithProviders(
      <As>
        <LabelSettings />
      </As>,
    );
    const card = await screen.findByRole("region", { name: "Label printer" });
    chooseOption(await within(card).findByLabelText("Printer"), "Xprinter XP-365B");
    const toggle = within(card).getByRole("switch", { name: "Print labels when stock arrives" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    expect(toggle).toBeChecked();
    fireEvent.click(within(card).getByRole("button", { name: "Save label printer" }));
    await waitFor(() =>
      expect(commands.setLabelPrinterProfile).toHaveBeenCalledWith(
        expect.objectContaining({ auto_print_on_receive: true }),
      ),
    );
  });

  it("opens on the saved sticker size and saves the chosen one as the automatic-printing default", async () => {
    vi.mocked(commands.labelTemplates).mockImplementation(() => ok([TWO_UP, TEMPLATE]));
    vi.mocked(commands.labelPrinterProfile).mockImplementation(() => ok(PROFILE));
    renderWithProviders(
      <As>
        <LabelSettings />
      </As>,
    );
    expect(await screen.findByRole("radio", { name: /30 x 16 mm, 3 across/ })).toBeChecked();
    expect(screen.getByText(/connected by Network/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: /50 x 25 mm, 2 across/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save label printer" }));
    // Darkness, speed, shifts and the network connection are no longer on the card, and saving keeps them.
    await waitFor(() =>
      expect(commands.setLabelPrinterProfile).toHaveBeenCalledWith({
        ...PROFILE,
        default_template_id: "two_up_50x25",
      }),
    );
  });

  it("switches a network printer to a Windows printer picked on the card", async () => {
    vi.mocked(commands.labelPrinterProfile).mockImplementation(() => ok(PROFILE));
    renderWithProviders(
      <As>
        <LabelSettings />
      </As>,
    );
    const card = await screen.findByRole("region", { name: "Label printer" });
    chooseOption(await within(card).findByLabelText("Printer"), "Xprinter XP-365B");
    expect(within(card).queryByText(/connected by Network/)).not.toBeInTheDocument();
    fireEvent.click(within(card).getByRole("button", { name: "Save label printer" }));
    await waitFor(() =>
      expect(commands.setLabelPrinterProfile).toHaveBeenCalledWith(
        expect.objectContaining({
          transport: { kind: "spooler", printer_name: "Xprinter XP-365B" },
          language: "tspl",
          darkness: 10,
          speed: 3,
        }),
      ),
    );
  });

  it("previews a sample sticker for the chosen roll beside the form", async () => {
    vi.mocked(commands.labelTemplates).mockImplementation(() => ok([TWO_UP, TEMPLATE]));
    renderWithProviders(
      <As>
        <LabelSettings />
      </As>,
    );
    const preview = await screen.findByRole("img", { name: "Sticker preview" });
    expect(preview).toHaveTextContent("Rs. 1,250");
    expect(commands.labelSamplePreview).toHaveBeenCalledWith("two_up_50x25");
    fireEvent.click(screen.getByRole("radio", { name: /30 x 16 mm, 3 across/ }));
    await waitFor(() => expect(commands.labelSamplePreview).toHaveBeenCalledWith("three_up_30x16"));
  });

  it("shows every setting on the card, nothing folded away, with the chosen size's calibration", async () => {
    vi.mocked(commands.labelTemplates).mockImplementation(() => ok([TWO_UP, TEMPLATE, THREE_UP_20]));
    renderWithProviders(
      <As>
        <LabelSettings />
      </As>,
    );
    expect(await screen.findByLabelText("Printer")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Advanced Calibration" })).not.toBeInTheDocument();
    // The printer's technical settings are not on the card: calibration per sticker size does the positioning.
    for (const gone of ["Connection", "Printer language", "Darkness (0–15)", "Speed (1–6)"]) {
      expect(screen.queryByLabelText(gone)).not.toBeInTheDocument();
    }
    expect(screen.queryByLabelText(/^Shift/)).not.toBeInTheDocument();
    // All three sticker sizes in one row.
    const sizes = screen.getByRole("radiogroup", { name: "Sticker size" });
    expect(within(sizes).getAllByRole("radio")).toHaveLength(3);
    expect(sizes).toHaveClass("sm:grid-cols-3");

    // The chosen size's three calibration fields (no row gap), then switching sizes shows that size's own
    // values, and an edit left unsaved on one size doesn't follow to another.
    const fields = ["Gap between labels (mm)", "Top margin (mm)", "Left margin (mm)"];
    const shown = () => fields.map((f) => (screen.getByLabelText(f) as HTMLInputElement).value);
    expect(shown()).toEqual(["3", "0.5", "1.5"]);
    expect(screen.queryByLabelText("Gap between rows (mm)")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Left margin (mm)"), { target: { value: "2" } });
    fireEvent.click(screen.getByRole("radio", { name: /30 x 16 mm, 3 across/ }));
    expect(shown()).toEqual(["2", "0", "1.5"]);

    // Every action sits in the footer; the calibration area has no buttons of its own.
    const calibration = screen.getByRole("group", { name: /30 x 16 mm/ });
    expect(within(calibration).queryAllByRole("button")).toHaveLength(0);
    for (const name of [
      "Save label printer",
      "Print Test Label",
      "Print alignment test",
      "Reset to default",
    ]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
  });

  it("chooses the sticker size, then prints, saves the calibration with the printer, and resets the roll", async () => {
    vi.mocked(commands.labelTemplates).mockImplementation(() => ok([TWO_UP, TEMPLATE]));
    renderWithProviders(
      <As>
        <LabelSettings />
      </As>,
    );
    const two = await screen.findByRole("radio", { name: /50 x 25 mm, 2 across/ });
    expect(two).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: /30 x 16 mm, 3 across/ }));
    expect(localStorage.getItem("gayanshop.label.template")).toBe("three_up_30x16");

    fireEvent.click(screen.getByRole("button", { name: "Print Test Label" }));
    await waitFor(() =>
      expect(commands.labelSamplePrint).toHaveBeenCalledWith("three_up_30x16", { ...STICKER, data: [] }),
    );
    expect(commands.labelAlignmentTest).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Print alignment test" }));
    await waitFor(() => expect(commands.labelAlignmentTest).toHaveBeenCalledWith("three_up_30x16"));
    fireEvent.change(screen.getByLabelText("Gap between labels (mm)"), { target: { value: "2.5" } });
    fireEvent.change(screen.getByLabelText("Top margin (mm)"), { target: { value: "1" } });
    // One Save: the printer profile, then the roll's calibration (its row gap kept as stored).
    fireEvent.click(screen.getByRole("button", { name: "Save label printer" }));
    await waitFor(() =>
      expect(commands.calibrateLabelTemplate).toHaveBeenCalledWith("three_up_30x16", {
        col_gap_mm: 2.5,
        row_gap_mm: 2,
        top_margin_mm: 1,
        left_margin_mm: 1.5,
      }),
    );
    expect(commands.setLabelPrinterProfile).toHaveBeenCalledWith(
      expect.objectContaining({ default_template_id: "three_up_30x16" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Reset to default" }));
    await waitFor(() =>
      expect(commands.calibrateLabelTemplate).toHaveBeenLastCalledWith("three_up_30x16", null),
    );
  });

  it("shows Rust's refusal of a calibration under the single Save", async () => {
    vi.mocked(commands.calibrateLabelTemplate).mockImplementation(() =>
      err({ kind: "VALIDATION", code: "LABEL_CALIBRATION_INVALID", detail: "0–10 mm" }),
    );
    renderWithProviders(
      <As>
        <LabelSettings />
      </As>,
    );
    fireEvent.change(await screen.findByLabelText("Left margin (mm)"), { target: { value: "9.5" } });
    fireEvent.click(screen.getByRole("button", { name: "Save label printer" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("between 0 and 10 mm");
    expect(commands.setLabelPrinterProfile).toHaveBeenCalled();
  });

  it("refuses a calibration that isn't a number before saving anything", async () => {
    renderWithProviders(
      <As>
        <LabelSettings />
      </As>,
    );
    fireEvent.change(await screen.findByLabelText("Top margin (mm)"), { target: { value: "abc" } });
    fireEvent.click(screen.getByRole("button", { name: "Save label printer" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(commands.setLabelPrinterProfile).not.toHaveBeenCalled();
    expect(commands.calibrateLabelTemplate).not.toHaveBeenCalled();
  });
});
