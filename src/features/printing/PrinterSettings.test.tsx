import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import {
  commands,
  type Perm,
  type ReceiptHeaderView_Serialize,
  type ReceiptPrinterProfile,
  type ReceiptPrinterProfile_Serialize,
} from "@/db/bindings";
import { chooseOption, err, ok, renderWithProviders, session } from "@/test/render";
import { usePrintReceiptStore } from "@/components/receipt/PrintableReceiptPortal";
import { DEFAULT_FORM, PrinterSettings, toForm, toProfile } from "./PrinterSettings";

vi.mock("@/db/bindings", () => ({
  commands: {
    printerProfile: vi.fn(),
    printerStatus: vi.fn(),
    setPrinterProfile: vi.fn(),
    printTestPage: vi.fn(),
    listSystemPrinters: vi.fn(),
    receiptHeader: vi.fn(),
    setReceiptLogo: vi.fn(),
  },
}));

const NET: ReceiptPrinterProfile_Serialize = {
  transport: { kind: "network", host: "192.168.1.50", port: 9100 },
  width: "mm80",
  codepage: "wpc1252",
  cut: "full",
  feed_lines: 3,
  qr: false,
  drawer_pin: "pin5",
  open_drawer: false,
  preview_before_print: false,
};

const HEADER: ReceiptHeaderView_Serialize = {
  logo: null,
  store_name: "Sale Center",
  footer: "Exchange within 7 days. Item for item only; no cash refunds. Thank you, come again!",
};

function As({ perms, children }: { perms: Perm[]; children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: perms })), [setSession, perms]);
  return <>{children}</>;
}

function renderAs(perms: Perm[] = ["SETTINGS_MANAGE"]) {
  renderWithProviders(
    <As perms={perms}>
      <PrinterSettings />
    </As>,
  );
}

beforeEach(() => {
  vi.mocked(commands.printerProfile).mockImplementation(() => ok(null));
  vi.mocked(commands.printerStatus).mockImplementation(() =>
    ok({ health: "not_configured" as const, fault: null, queued: 0, failed: 0, recovered: 0 }),
  );
  vi.mocked(commands.setPrinterProfile).mockImplementation(() => ok(null));
  vi.mocked(commands.printTestPage).mockImplementation(() => ok(null));
  vi.mocked(commands.receiptHeader).mockImplementation(() => ok(HEADER));
  vi.mocked(commands.setReceiptLogo).mockImplementation(() => ok(null));
  vi.mocked(commands.listSystemPrinters).mockImplementation(() => ok(["XP-80C", "Microsoft Print to PDF"]));
});

describe("printer form mapping", () => {
  it("round-trips every transport", () => {
    expect(toProfile(toForm(NET))).toEqual(NET);
    const usb: ReceiptPrinterProfile = { ...NET, transport: { kind: "spooler", printer_name: "XP-80C" } };
    expect(toProfile(toForm(usb))).toEqual(usb);
    const com: ReceiptPrinterProfile = { ...NET, transport: { kind: "serial", port: "COM3", baud: 19200 } };
    expect(toProfile(toForm(com))).toEqual(com);
  });

  it("defaults to an 80 mm USB printer with a partial cut and a barcode", () => {
    expect(toProfile(DEFAULT_FORM)).toEqual({
      transport: { kind: "spooler", printer_name: "" },
      width: "mm80",
      codepage: "pc437",
      cut: "partial",
      feed_lines: 4,
      qr: false,
      drawer_pin: "pin2",
      open_drawer: true,
      preview_before_print: false,
    });
  });

  it("sends a non-numeric port as 0 so Rust rejects it", () => {
    const p = toProfile({ ...DEFAULT_FORM, kind: "network", host: "printer", port: "91x" });
    expect(p.transport).toEqual({ kind: "network", host: "printer", port: 0 });
  });
});

describe("PrinterSettings", () => {
  it("saves printer settings and prints the test page", async () => {
    renderAs();
    expect(await screen.findByText("Not set up")).toBeInTheDocument();
    chooseOption(await screen.findByLabelText("Printer"), "XP-80C");
    fireEvent.click(screen.getByRole("checkbox", { name: /Print a QR code/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(commands.setPrinterProfile).toHaveBeenCalledWith({
        ...toProfile(DEFAULT_FORM),
        transport: { kind: "spooler", printer_name: "XP-80C" },
        qr: true,
      }),
    );
    expect(await screen.findByText("Printer settings saved.")).toBeInTheDocument();
  });

  it("toggles receipt print preview and saves it to profile and localStorage", async () => {
    renderAs();
    const previewSwitch = await screen.findByRole("switch", { name: "Receipt print preview" });
    expect(previewSwitch).not.toBeChecked();
    fireEvent.click(previewSwitch);
    expect(previewSwitch).toBeChecked();
    expect(localStorage.getItem("pos_preview_receipt")).toBe("true");

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(commands.setPrinterProfile).toHaveBeenCalledWith(
        expect.objectContaining({
          preview_before_print: true,
        }),
      ),
    );
  });

  it("loads the saved profile and prints the sample test receipt", async () => {
    vi.mocked(commands.printerProfile).mockImplementation(() => ok(NET));
    const spy = vi.spyOn(usePrintReceiptStore.getState(), "print").mockImplementation(() => undefined);
    renderAs();
    fireEvent.click(await screen.findByRole("button", { name: "Print Test Receipt" }));
    expect(spy).toHaveBeenCalledWith(
      expect.objectContaining({
        order_id: "SAMPLE-RECEIPT",
        receipt_no: "T01-20261120-0001",
      }),
    );
  });

  it("previews the sample receipt beside the form with the shop name and footer", async () => {
    renderAs();
    const preview = await screen.findByRole("img", { name: "Receipt preview" });
    expect(await within(preview).findByText("Sale Center")).toBeInTheDocument();
    expect(within(preview).getByText("*** SAMPLE RECEIPT - NOT A SALE ***")).toBeInTheDocument();
    expect(within(preview).getByText(HEADER.footer)).toBeInTheDocument();
  });

  it("offers to choose a header image while none is saved", async () => {
    renderAs();
    expect(await screen.findByRole("button", { name: "Choose image" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove image" })).not.toBeInTheDocument();
  });

  it("removes a saved header image", async () => {
    vi.mocked(commands.receiptHeader).mockImplementation(() =>
      ok({ ...HEADER, logo: { width: 8, height: 1, data: [0xff] } }),
    );
    renderAs();
    fireEvent.click(await screen.findByRole("button", { name: "Remove image" }));
    await waitFor(() => expect(commands.setReceiptLogo).toHaveBeenCalledWith(null));
    expect(screen.getByRole("button", { name: "Replace image" })).toBeInTheDocument();
  });

  it("shows zoom controls when a header image is saved", async () => {
    vi.mocked(commands.receiptHeader).mockImplementation(() =>
      ok({ ...HEADER, logo: { width: 384, height: 100, data: new Array(48 * 100).fill(0) } }),
    );
    renderAs();
    expect(await screen.findByRole("button", { name: "Zoom out" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Zoom in" })).toBeInTheDocument();
    expect(screen.getByRole("slider", { name: "Header image size" })).toBeInTheDocument();
  });

  it("shows Rust's validation message", async () => {
    vi.mocked(commands.setPrinterProfile).mockImplementation(() =>
      err({ kind: "VALIDATION", code: "PRINTER_PROFILE_INVALID", detail: "host or port" }),
    );
    renderAs();
    await screen.findByText("Not set up");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Check the printer connection details.");
  });

  it("picks the printer from the installed ones, refreshes the list and saves the drawer switch", async () => {
    renderAs();
    await screen.findByText("Not set up");
    chooseOption(await screen.findByLabelText("Printer"), "XP-80C");
    vi.mocked(commands.listSystemPrinters).mockImplementation(() => ok(["XP-80C", "Zebra ZD220"]));
    fireEvent.click(screen.getByRole("button", { name: "Refresh printer list" }));
    await waitFor(() => expect(commands.listSystemPrinters).toHaveBeenCalledTimes(2));
    const drawer = screen.getByRole("switch", { name: "Open cash drawer" });
    expect(drawer).toBeChecked();
    fireEvent.click(drawer);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(commands.setPrinterProfile).toHaveBeenCalledWith({
        ...toProfile(DEFAULT_FORM),
        transport: { kind: "spooler", printer_name: "XP-80C" },
        open_drawer: false,
      }),
    );
  });

  it("keeps a saved printer that is no longer installed in the list", async () => {
    vi.mocked(commands.printerProfile).mockImplementation(() =>
      ok({ ...NET, transport: { kind: "spooler", printer_name: "Old Printer" } }),
    );
    renderAs();
    const select = await screen.findByRole("combobox", { name: "Printer" });
    await waitFor(() => expect(select).toHaveTextContent("Old Printer (not installed)"));
  });

  it("explains an empty printer list", async () => {
    vi.mocked(commands.listSystemPrinters).mockImplementation(() => ok([]));
    renderAs();
    expect(await screen.findByText(/No printers found/)).toBeInTheDocument();
  });

  it("shows cash drawer pin and QR checkbox directly on the card without Advanced Settings", async () => {
    renderAs();
    await screen.findByText("Not set up");
    expect(screen.queryByRole("button", { name: "Advanced Settings" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Cut")).not.toBeInTheDocument();
    expect(screen.queryByRole("radio", { name: /80 mm/ })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Printer")).toBeInTheDocument();
    expect(screen.getByLabelText("Cash drawer pin")).toBeInTheDocument();
    expect(screen.getByLabelText(/Print a QR code/)).toBeInTheDocument();
  });

  it("explains when the printer is not a Windows printer", async () => {
    vi.mocked(commands.printerProfile).mockImplementation(() => ok(NET));
    renderAs();
    expect(await screen.findByText(/connected by Network \(TCP 9100\)/)).toBeInTheDocument();
    expect(screen.queryByLabelText("Printer")).not.toBeInTheDocument();
  });

  it("hides the actions without SETTINGS_MANAGE", async () => {
    renderAs(["SALE_CREATE"]);
    await screen.findByText("Not set up");
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
  });
});
