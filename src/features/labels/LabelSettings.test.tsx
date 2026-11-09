import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import {
  commands,
  type LabelPrinterProfile,
  type LabelTemplate_Serialize as LabelTemplate,
  type Perm,
} from "@/db/bindings";
import { ok, renderWithProviders, session } from "@/test/render";
import { LabelSettings, parseMm, toForm, toProfile } from "./LabelSettings";

vi.mock("@/db/bindings", () => ({
  commands: {
    labelPrinterProfile: vi.fn(),
    labelTemplates: vi.fn(),
    setLabelPrinterProfile: vi.fn(),
    calibrateLabelTemplate: vi.fn(),
    labelAlignmentTest: vi.fn(),
  },
}));

const TEMPLATE = {
  id: "three_up_1x1",
  name: "3-up",
  columns: 3,
  label_w_mm: 25.4,
  label_h_mm: 25.4,
  col_gap_mm: 2,
  row_gap_mm: 2,
  left_margin_mm: 1.5,
  dpi: 203,
  language: "tspl",
  symbology: "code128",
  elements: [],
} as unknown as LabelTemplate;

const PROFILE: LabelPrinterProfile = {
  transport: { kind: "network", host: "192.168.1.60", port: 9100 },
  language: "zpl",
  dpi: 300,
  darkness: 10,
  speed: 3,
  offset_x_mm: 0.5,
  offset_y_mm: -0.25,
};

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
  it("saves a TSPL USB label printer", async () => {
    renderWithProviders(
      <As>
        <LabelSettings />
      </As>,
    );
    const card = await screen.findByRole("form", { name: "Label printer" });
    fireEvent.change(within(card).getByLabelText("Windows printer name"), {
      target: { value: "Xprinter XP-365B" },
    });
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

  it("calibrates a roll, prints the alignment test and resets", async () => {
    renderWithProviders(
      <As>
        <LabelSettings />
      </As>,
    );
    const card = await screen.findByRole("form", { name: '3 across, 1" x 1"' });
    fireEvent.change(within(card).getByLabelText("Gap between labels (mm)"), { target: { value: "2.5" } });
    fireEvent.click(within(card).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(commands.calibrateLabelTemplate).toHaveBeenCalledWith("three_up_1x1", {
        col_gap_mm: 2.5,
        row_gap_mm: 2,
        left_margin_mm: 1.5,
      }),
    );
    fireEvent.click(within(card).getByRole("button", { name: "Print alignment test" }));
    await waitFor(() => expect(commands.labelAlignmentTest).toHaveBeenCalledWith("three_up_1x1"));
    fireEvent.click(within(card).getByRole("button", { name: "Reset to default" }));
    await waitFor(() =>
      expect(commands.calibrateLabelTemplate).toHaveBeenLastCalledWith("three_up_1x1", null),
    );
  });
});
