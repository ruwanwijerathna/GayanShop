import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/db/bindings";
import { err, ok, renderWithProviders } from "@/test/render";
import { RestoreRecovery } from "./RestoreRecovery";
import { RestoreSettings } from "./RestoreSettings";

vi.mock("@/db/bindings", () => ({
  commands: {
    restoreCandidates: vi.fn(),
    restorePickFile: vi.fn(),
    restoreStage: vi.fn(),
    restoreStatus: vi.fn(),
    restoreRecover: vi.fn(),
    listLoginStaff: vi.fn(),
  },
}));

const inspected = {
  path: "D:\\usb\\GayanShop-T01-20261108.db",
  store_id: "s",
  device_id: "d",
  device_code: "T01",
  schema_version: 3,
  taken_at: "2026-11-08T05:00:00.000Z",
  size_bytes: 1_048_576,
  sha256: "ab",
};

beforeEach(() => {
  for (const f of Object.values(commands)) vi.mocked(f).mockReset();
});

describe("RestoreSettings", () => {
  it("restores an automatic backup after a clear confirmation", async () => {
    vi.mocked(commands.restoreCandidates).mockImplementation(() =>
      ok([
        {
          path: "C:\\data\\backups\\auto\\auto-T01-20261109-100000.db",
          taken_at: "2026-11-09T04:30:00.000Z",
          size_bytes: 2_097_152,
          kind: "auto",
        },
      ]),
    );
    vi.mocked(commands.restoreStage).mockImplementation(() => ok(inspected));
    renderWithProviders(<RestoreSettings />);
    fireEvent.click(await screen.findByRole("button", { name: /Automatic/ }));
    expect(screen.getByRole("group", { name: "Restore this backup?" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Restore and restart" }));
    await waitFor(() =>
      expect(commands.restoreStage).toHaveBeenCalledWith(
        "C:\\data\\backups\\auto\\auto-T01-20261109-100000.db",
      ),
    );
    expect(await screen.findByText("The backup is ready. The app is restarting…")).toBeInTheDocument();
  });

  it("explains a refused backup", async () => {
    vi.mocked(commands.restoreCandidates).mockImplementation(() => ok([]));
    vi.mocked(commands.restorePickFile).mockImplementation(() => ok("D:\\usb\\t02.db"));
    vi.mocked(commands.restoreStage).mockImplementation(() =>
      err({ kind: "VALIDATION", code: "RESTORE_OTHER_COUNTER", detail: "T02" }),
    );
    renderWithProviders(<RestoreSettings />);
    expect(await screen.findByText("No backups on this counter yet.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Choose a backup file…" }));
    fireEvent.click(await screen.findByRole("button", { name: "Restore and restart" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This backup belongs to another counter (T02). Restore it on that counter.",
    );
  });
});

describe("RestoreRecovery", () => {
  it("finishes the restore online with the Owner's PIN", async () => {
    vi.mocked(commands.restoreStatus).mockImplementation(() =>
      ok({
        pending: {
          source: inspected.path,
          source_sha256: "ab",
          taken_at: "2026-11-08T05:00:00.000Z",
          pre_restore: "x",
          requested_by: "o",
          requested_at: "2026-11-09T05:00:00.000Z",
        },
        modes: ["online", "offline"],
        device_code: "T01",
      }),
    );
    vi.mocked(commands.listLoginStaff).mockImplementation(() =>
      ok([
        { id: "o", display_name: "Gayan", role_name: "Owner" },
        { id: "c", display_name: "Nimal", role_name: "Cashier" },
      ]),
    );
    vi.mocked(commands.restoreRecover).mockImplementation(() =>
      ok({ mode: "online", device_code: "T01", rows_recovered: 42 }),
    );
    renderWithProviders(<RestoreRecovery />);
    expect(await screen.findByRole("heading", { name: "Finish the restore" })).toBeInTheDocument();
    fireEvent.click(
      await screen.findByRole("button", { name: /Bring back this counter's sales from the cloud/ }),
    );
    expect(screen.queryByRole("button", { name: /Continue from the backup/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Nimal/ })).not.toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: /Gayan/ }));
    const pin = screen.getByLabelText(/Gayan/);
    fireEvent.change(pin, { target: { value: "246810" } });
    fireEvent.submit(pin.closest("form") as HTMLFormElement);
    await waitFor(() =>
      expect(commands.restoreRecover).toHaveBeenCalledWith({ mode: "online", staff_id: "o", pin: "246810" }),
    );
  });

  it("tells the Owner when the cloud's chain disagrees, so the offline path is the way on", async () => {
    vi.mocked(commands.restoreStatus).mockImplementation(() =>
      ok({ pending: null, modes: ["online", "offline"], device_code: "T01" }),
    );
    vi.mocked(commands.listLoginStaff).mockImplementation(() =>
      ok([{ id: "o", display_name: "Gayan", role_name: "Owner" }]),
    );
    vi.mocked(commands.restoreRecover).mockImplementation(() =>
      err({ kind: "INVARIANT", code: "RESTORE_CHAIN_MISMATCH", detail: "x" }),
    );
    renderWithProviders(<RestoreRecovery />);
    fireEvent.click(await screen.findByRole("button", { name: /Bring back/ }));
    fireEvent.click(await screen.findByRole("button", { name: /Gayan/ }));
    const pin = screen.getByLabelText(/Gayan/);
    fireEvent.change(pin, { target: { value: "246810" } });
    fireEvent.submit(pin.closest("form") as HTMLFormElement);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Finish the restore as a new counter instead.",
    );
  });
});
