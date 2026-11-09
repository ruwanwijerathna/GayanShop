import { fireEvent, screen, waitFor } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import { commands, type Perm } from "@/db/bindings";
import { err, ok, renderWithProviders, session } from "@/test/render";
import { BackupSettings, relativeTime } from "./BackupSettings";

vi.mock("@/lib/audio", () => ({ playCue: vi.fn(), setAudioPrefs: vi.fn() }));
vi.mock("@/db/bindings", () => ({
  commands: { backupStatus: vi.fn(), backupPickFolder: vi.fn(), backupNow: vi.fn() },
}));

function As({ perms, children }: { perms: Perm[]; children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: perms })), [setSession, perms]);
  return <>{children}</>;
}

function renderAs(perms: Perm[] = ["BACKUP_CREATE"]) {
  renderWithProviders(
    <As perms={perms}>
      <BackupSettings />
    </As>,
  );
}

beforeEach(() => {
  vi.mocked(commands.backupStatus).mockImplementation(() =>
    ok({
      last_at: null,
      last_path: null,
      last_dir: null,
      reminder_due: true,
      last_auto_at: null,
      auto_error: null,
    }),
  );
  vi.mocked(commands.backupPickFolder).mockImplementation(() => ok("E:\\"));
  vi.mocked(commands.backupNow).mockImplementation(() =>
    ok({
      path: "E:\\GayanShop-T01-20261108-143000.db",
      manifest_path: "E:\\GayanShop-T01-20261108-143000.db.json",
      size_bytes: 1_048_576,
      sha256: "ab".repeat(32),
      created_at: "2026-11-08T09:00:00.000Z",
    }),
  );
});

describe("BackupSettings", () => {
  it("picks a folder, backs up and shows the path and hash", async () => {
    renderAs();
    expect(await screen.findByText("No backup has been taken on this counter yet.")).toBeInTheDocument();
    expect(screen.getByText(/Keep this drive safe/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Backup Database Now" }));
    await waitFor(() => expect(commands.backupNow).toHaveBeenCalledWith("E:\\"));
    expect(await screen.findByText("Backup saved: E:\\GayanShop-T01-20261108-143000.db")).toBeInTheDocument();
    expect(screen.getByText(`SHA-256: ${"ab".repeat(32)}`)).toBeInTheDocument();
  });

  it("does nothing when the picker is cancelled", async () => {
    vi.mocked(commands.backupPickFolder).mockImplementation(() => ok(null));
    renderAs();
    fireEvent.click(await screen.findByRole("button", { name: "Backup Database Now" }));
    await waitFor(() => expect(commands.backupPickFolder).toHaveBeenCalled());
    expect(commands.backupNow).not.toHaveBeenCalled();
  });

  it("explains a failure and says sales are safe", async () => {
    vi.mocked(commands.backupNow).mockImplementation(() =>
      err({ kind: "VALIDATION", code: "BACKUP_WRITE_FAILED", detail: "gone" }),
    );
    renderAs();
    fireEvent.click(await screen.findByRole("button", { name: "Backup Database Now" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Was the drive removed?");
  });

  it("hides the button without BACKUP_CREATE", async () => {
    renderAs(["SALE_CREATE"]);
    await screen.findByText(/No backup has been taken/);
    expect(screen.queryByRole("button", { name: "Backup Database Now" })).not.toBeInTheDocument();
  });

  it("says when the last backup was", () => {
    const now = Date.parse("2026-11-08T11:00:00.000Z");
    expect(relativeTime("2026-11-08T09:00:00.000Z", now, "en")).toBe("2 hours ago");
    expect(relativeTime("2026-11-01T11:00:00.000Z", now, "en")).toBe("7 days ago");
  });
});
