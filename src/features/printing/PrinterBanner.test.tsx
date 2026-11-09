import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, type PrinterStatusView_Serialize as PrinterStatusView } from "@/db/bindings";
import { ok, renderWithProviders } from "@/test/render";
import { PrinterBanner } from "./PrinterBanner";

vi.mock("@/db/bindings", () => ({
  commands: {
    printerStatus: vi.fn(),
    printRetry: vi.fn(),
    printDismissFailed: vi.fn(),
    printRecoveryResolve: vi.fn(),
  },
}));

const READY: PrinterStatusView = { health: "ready", fault: null, queued: 0, failed: 0, recovered: 0 };

function status(s: Partial<PrinterStatusView>) {
  vi.mocked(commands.printerStatus).mockImplementation(() => ok({ ...READY, ...s }));
}

beforeEach(() => {
  vi.mocked(commands.printRetry).mockImplementation(() => ok(null));
  vi.mocked(commands.printDismissFailed).mockImplementation(() => ok(null));
  vi.mocked(commands.printRecoveryResolve).mockImplementation(() => ok(null));
});

describe("PrinterBanner", () => {
  it("shows nothing while the printer is ready", async () => {
    status({});
    renderWithProviders(<PrinterBanner />);
    await waitFor(() => expect(commands.printerStatus).toHaveBeenCalled());
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("reports paper out with the waiting count and retries", async () => {
    status({ health: "fault", fault: "PAPER_OUT", failed: 2 });
    renderWithProviders(<PrinterBanner />);
    expect(await screen.findByText("Printer: paper out — 2 receipt(s) waiting.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(commands.printRetry).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "Reprint later" }));
    await waitFor(() => expect(commands.printDismissFailed).toHaveBeenCalledTimes(1));
  });

  it("asks before printing receipts left from before a restart", async () => {
    status({ recovered: 3 });
    renderWithProviders(<PrinterBanner />);
    expect(
      await screen.findByText("3 receipt(s) from before the restart were not printed."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Print now" }));
    await waitFor(() => expect(commands.printRecoveryResolve).toHaveBeenCalledWith(true));
    fireEvent.click(screen.getByRole("button", { name: "Skip" }));
    await waitFor(() => expect(commands.printRecoveryResolve).toHaveBeenCalledWith(false));
  });

  it("tells the cashier to use the drawer key while the printer hangs", async () => {
    status({ health: "unhealthy", fault: "TIMEOUT", queued: 1 });
    renderWithProviders(<PrinterBanner />);
    expect(await screen.findByText(/not responding\. Use the drawer key/)).toBeInTheDocument();
  });

  it("explains that no printer is set up", async () => {
    status({ health: "not_configured", queued: 2 });
    renderWithProviders(<PrinterBanner />);
    expect(
      await screen.findByText("No receipt printer is set up — 2 receipt(s) waiting."),
    ).toBeInTheDocument();
  });
});
