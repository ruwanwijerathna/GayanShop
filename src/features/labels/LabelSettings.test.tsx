import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import {
  commands,
  type LabelPrinterProfile,
  type LabelPreview_Serialize as LabelPreview,
  type LabelTemplate_Serialize as LabelTemplate,
  type Perm,
} from "@/db/bindings";
import { chooseOption, ok, renderWithProviders, session } from "@/test/render";
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

const TEMPLATE = {
  id: "three_up_30x16",
  name: "3-up",
  columns: 3,
  label_w_mm: 30,
  label_h_mm: 16,
  col_gap_mm: 2,
  row_gap_mm: 2,
  left_margin_mm: 1.5,
  dpi: 203,
  language: "tspl",
  symbology: "code128",
  elements: [],
} as unknown as LabelTemplate;

const TWO_UP = { ...TEMPLATE, id: "two_up_50x25", name: "2-up", columns: 2 } as unknown as LabelTemplate;

const PROFILE: LabelPrinterProfile = {
  transport: { kind: "network", host: "192.168.1.60", port: 9100 },
  language: "zpl",
  dpi: 300,
  darkness: 10,
  speed: 3,
  offset_x_mm: 0.5,
  offset_y_mm: -0.25,
};

const SAMPLE = {
  template_id: "three_up_30x16",
  dpi: 203,
  columns: 3,
  rows: 1,
  roll_width_mm: 97,
  label_w_mm: 30,
  label_h_mm: 16,
  row_gap_mm: 3,
  labels: [
    {
      row: 0,
      col: 0,
      product_id: "sample",
      x_mm: 1.5,
      texts: [
        {
          kind: "price",
          x_mm: 2.5,
          y_mm: 10.4,
          w_mm: 28,
          h_mm: 4,
          font_size: 4,
          bold: true,
          align: "center",
          lines: ["Rs. 1,250"],
        },
      ],
      barcode: null,
    },
  ],
  warnings: [],
} as unknown as LabelPreview;

const MANAGER: Perm[] = ["SETTINGS_MANAGE"];

function As({ children }: { children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: MANAGER })), [setSession]);
  return <>{children}</>;
}

beforeEach(() => {
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
      }),
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

  it("keeps calibration in a closed Advanced Calibration section", async () => {
    renderWithProviders(
      <As>
        <LabelSettings />
      </As>,
    );
    const trigger = await screen.findByRole("button", { name: "Advanced Calibration" });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByLabelText("Darkness (0–15)")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Gap between labels (mm)")).not.toBeInTheDocument();
    fireEvent.click(trigger);
    expect(await screen.findByLabelText("Darkness (0–15)")).toBeInTheDocument();
    expect(screen.getByLabelText("Gap between labels (mm)")).toBeInTheDocument();
  });

  it("chooses the sticker size, remembers it, and tests, calibrates and resets that roll", async () => {
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
    await waitFor(() => expect(commands.labelSamplePrint).toHaveBeenCalledWith("three_up_30x16"));
    expect(commands.labelAlignmentTest).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Advanced Calibration" }));
    fireEvent.click(await screen.findByRole("button", { name: "Print alignment test" }));
    await waitFor(() => expect(commands.labelAlignmentTest).toHaveBeenCalledWith("three_up_30x16"));
    fireEvent.change(await screen.findByLabelText("Gap between labels (mm)"), { target: { value: "2.5" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(commands.calibrateLabelTemplate).toHaveBeenCalledWith("three_up_30x16", {
        col_gap_mm: 2.5,
        row_gap_mm: 2,
        left_margin_mm: 1.5,
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Reset to default" }));
    await waitFor(() =>
      expect(commands.calibrateLabelTemplate).toHaveBeenLastCalledWith("three_up_30x16", null),
    );
  });
});
