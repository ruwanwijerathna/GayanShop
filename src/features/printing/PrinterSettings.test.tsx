import { fireEvent, screen, waitFor } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import { commands, type Perm, type ReceiptPrinterProfile } from "@/db/bindings";
import { chooseOption, err, ok, renderWithProviders, session } from "@/test/render";
import { DEFAULT_FORM, PrinterSettings, toForm, toProfile } from "./PrinterSettings";

vi.mock("@/db/bindings", () => ({
  commands: {
    printerProfile: vi.fn(),
    printerStatus: vi.fn(),
    setPrinterProfile: vi.fn(),
    printTestPage: vi.fn(),
  },
}));

const NET: ReceiptPrinterProfile = {
  transport: { kind: "network", host: "192.168.1.50", port: 9100 },
  width: "mm58",
  codepage: "wpc1252",
  cut: "full",
  feed_lines: 3,
  qr: false,
  drawer_pin: "pin5",
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
});

describe("printer form mapping", () => {
  it("round-trips every transport", () => {
    expect(toProfile(toForm(NET))).toEqual(NET);
    const usb: ReceiptPrinterProfile = { ...NET, transport: { kind: "spooler", printer_name: "XP-80C" } };
    expect(toProfile(toForm(usb))).toEqual(usb);
    const com: ReceiptPrinterProfile = { ...NET, transport: { kind: "serial", port: "COM3", baud: 19200 } };
    expect(toProfile(toForm(com))).toEqual(com);
  });

  it("defaults to an 80 mm USB printer with a partial cut and a QR", () => {
    expect(toProfile(DEFAULT_FORM)).toEqual({
      transport: { kind: "spooler", printer_name: "" },
      width: "mm80",
      codepage: "pc437",
      cut: "partial",
      feed_lines: 4,
      qr: true,
      drawer_pin: "pin2",
    });
  });

  it("sends a non-numeric port as 0 so Rust rejects it", () => {
    const p = toProfile({ ...DEFAULT_FORM, kind: "network", host: "printer", port: "91x" });
    expect(p.transport).toEqual({ kind: "network", host: "printer", port: 0 });
  });
});

describe("PrinterSettings", () => {
  it("saves a network printer and prints the test page", async () => {
    renderAs();
    expect(await screen.findByText("Not set up")).toBeInTheDocument();
    chooseOption(await screen.findByLabelText("Connection"), "Network (TCP 9100)");
    fireEvent.change(screen.getByLabelText("IP address or host name"), { target: { value: "192.168.1.50" } });
    chooseOption(screen.getByLabelText("Paper width"), "58 mm (32 columns)");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(commands.setPrinterProfile).toHaveBeenCalledWith({
        ...toProfile(DEFAULT_FORM),
        transport: { kind: "network", host: "192.168.1.50", port: 9100 },
        width: "mm58",
      }),
    );
    expect(await screen.findByText("Printer settings saved.")).toBeInTheDocument();
  });

  it("loads the saved profile and queues a test page", async () => {
    vi.mocked(commands.printerProfile).mockImplementation(() => ok(NET));
    renderAs();
    expect(await screen.findByDisplayValue("192.168.1.50")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Print test page" }));
    await waitFor(() => expect(commands.printTestPage).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("Test page sent to the printer.")).toBeInTheDocument();
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

  it("hides the actions without SETTINGS_MANAGE", async () => {
    renderAs(["SALE_CREATE"]);
    await screen.findByText("Not set up");
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
  });
});
