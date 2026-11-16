import { screen } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import { commands, type Perm } from "@/db/bindings";
import { ok, openTab, renderWithProviders, session } from "@/test/render";
import { SettingsPage } from "./SettingsPage";

vi.mock("@/features/backup/BackupSettings", () => ({ BackupSettings: () => <p>backup-panel</p> }));
vi.mock("@/features/backup/RestoreSettings", () => ({ RestoreSettings: () => <p>restore-panel</p> }));
vi.mock("@/features/sync/SyncSettings", () => ({ SyncSettings: () => <p>sync-panel</p> }));
vi.mock("@/features/reports/MaintenanceSettings", () => ({
  MaintenanceSettings: () => <p>maintenance-panel</p>,
}));
vi.mock("@/components/common/AppearanceSettings", () => ({
  AppearanceSettings: () => <p>appearance-panel</p>,
}));
vi.mock("@/components/common/TouchModeSettings", () => ({ TouchModeSettings: () => <p>touch-panel</p> }));
vi.mock("@/db/bindings", () => ({
  commands: {
    printerProfile: vi.fn(),
    printerStatus: vi.fn(),
    labelPrinterProfile: vi.fn(),
    labelTemplates: vi.fn(),
    listSystemPrinters: vi.fn(),
  },
}));

function As({ perms, children }: { perms: Perm[]; children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: perms })), [setSession, perms]);
  return <>{children}</>;
}

function renderAs(perms: Perm[]) {
  renderWithProviders(
    <As perms={perms}>
      <SettingsPage />
    </As>,
  );
}

beforeEach(() => {
  vi.mocked(commands.printerProfile).mockImplementation(() => ok(null));
  vi.mocked(commands.printerStatus).mockImplementation(() =>
    ok({ health: "not_configured" as const, fault: null, queued: 0, failed: 0, recovered: 0 }),
  );
  vi.mocked(commands.labelPrinterProfile).mockImplementation(() => ok(null));
  vi.mocked(commands.labelTemplates).mockImplementation(() => ok([]));
  vi.mocked(commands.listSystemPrinters).mockImplementation(() => ok([]));
});

describe("SettingsPage", () => {
  it("opens on the Printers tab and shows the bill printer", async () => {
    renderAs(["SETTINGS_MANAGE", "BACKUP_RESTORE"]);
    expect(await screen.findByRole("tab", { name: "Printers", selected: true })).toBeInTheDocument();
    expect(
      await screen.findByRole("heading", { name: "Receipt printer and cash drawer" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("backup-panel")).not.toBeInTheDocument();
  });

  it("stacks the Label printer card below the Receipt printer card", async () => {
    const roll = { id: "two_up_50x25", name: "2-up", columns: 2, label_w_mm: 50, label_h_mm: 25 };
    vi.mocked(commands.labelTemplates).mockImplementation(() => ok([roll] as never));
    renderAs(["SETTINGS_MANAGE", "BACKUP_RESTORE"]);
    const receipt = await screen.findByRole("heading", { name: "Receipt printer and cash drawer" });
    const label = await screen.findByRole("heading", { name: "Label printer" });
    expect(receipt.compareDocumentPosition(label) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(receipt.closest("[role=tabpanel]")?.className).toContain("flex-col");
  });

  it("switches between the sections", async () => {
    renderAs(["SETTINGS_MANAGE", "BACKUP_RESTORE"]);
    openTab(await screen.findByRole("tab", { name: "General" }));
    expect(await screen.findByText("appearance-panel")).toBeInTheDocument();
    expect(screen.getByText("touch-panel")).toBeInTheDocument();
    openTab(screen.getByRole("tab", { name: "Backup" }));
    expect(await screen.findByText("backup-panel")).toBeInTheDocument();
    expect(screen.getByText("restore-panel")).toBeInTheDocument();
    openTab(screen.getByRole("tab", { name: "Sync" }));
    expect(await screen.findByText("sync-panel")).toBeInTheDocument();
    openTab(screen.getByRole("tab", { name: "Maintenance" }));
    expect(await screen.findByText("maintenance-panel")).toBeInTheDocument();
  });

  it("hides Maintenance and Restore without the permissions", async () => {
    renderAs(["SALE_CREATE"]);
    await screen.findByRole("tab", { name: "Printers" });
    expect(screen.queryByRole("tab", { name: "Maintenance" })).not.toBeInTheDocument();
    openTab(screen.getByRole("tab", { name: "Backup" }));
    expect(await screen.findByText("backup-panel")).toBeInTheDocument();
    expect(screen.queryByText("restore-panel")).not.toBeInTheDocument();
  });
});
