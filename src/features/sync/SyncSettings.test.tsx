import { fireEvent, screen, waitFor } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import { commands, type Perm } from "@/db/bindings";
import { err, ok, renderWithProviders, session } from "@/test/render";
import { SyncSettings } from "./SyncSettings";

vi.mock("@/db/bindings", () => ({
  commands: {
    syncStatus: vi.fn(),
    syncInfo: vi.fn(),
    syncLogs: vi.fn(),
    syncNow: vi.fn(),
    syncRegister: vi.fn(),
    syncDeadOps: vi.fn(),
    syncResolveDeadOp: vi.fn(),
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
      <SyncSettings />
    </As>,
  );
}

const status = (state: "synced" | "unregistered" | "pending", pending = 0, dead = 0) =>
  ok({ state, pending, dead, last_success_at: null, last_error: null, chain_broken: [] });

beforeEach(() => {
  for (const f of Object.values(commands)) vi.mocked(f).mockReset();
  vi.mocked(commands.syncLogs).mockImplementation(() => ok([]));
  vi.mocked(commands.syncDeadOps).mockImplementation(() => ok([]));
  vi.mocked(commands.syncNow).mockImplementation(() => ok(null));
});

describe("SyncSettings", () => {
  it("registers this counter with the owner's cloud account and forgets the password", async () => {
    vi.mocked(commands.syncStatus).mockImplementation(() => status("unregistered", 12));
    vi.mocked(commands.syncInfo).mockImplementation(() =>
      ok({ registered: false, server_url: null, registered_at: null, device_code: "T01" }),
    );
    vi.mocked(commands.syncRegister).mockImplementation(() =>
      ok({ store_id: "s", store_name: "GayanShop", device_id: "d", device_code: "T01" }),
    );
    renderAs(["SYNC_MANAGE"]);
    const type = (label: string, value: string) =>
      fireEvent.change(screen.getByLabelText(label), { target: { value } });
    type("Server address", "https://example.supabase.co");
    type("Project key (anon / public)", "anon-key");
    type("Owner's cloud email", "owner@gayanshop.lk");
    type("Owner's cloud password", "secret-1");
    fireEvent.click(screen.getByRole("button", { name: "Register" }));
    expect(await screen.findByText("Registered as T01.")).toBeInTheDocument();
    expect(commands.syncRegister).toHaveBeenCalledWith({
      server_url: "https://example.supabase.co",
      anon_key: "anon-key",
      owner_email: "owner@gayanshop.lk",
      owner_password: "secret-1",
      device_name: null,
    });
    expect(screen.getByLabelText("Owner's cloud password")).toHaveValue("");
    expect(screen.getByText("Waiting to send: 12")).toBeInTheDocument();
  });

  it("shows the server's refusal in the staff member's words", async () => {
    vi.mocked(commands.syncStatus).mockImplementation(() => status("unregistered"));
    vi.mocked(commands.syncInfo).mockImplementation(() =>
      ok({ registered: false, server_url: null, registered_at: null, device_code: "T01" }),
    );
    vi.mocked(commands.syncRegister).mockImplementation(() =>
      err({ kind: "VALIDATION", code: "SYNC_INVALID_CREDENTIALS", detail: "x" }),
    );
    renderAs(["SYNC_MANAGE"]);
    fireEvent.click(screen.getByRole("button", { name: "Register" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The email or password is wrong.");
  });

  it("lets the owner retry or discard a refused change, and anyone sync now", async () => {
    vi.mocked(commands.syncStatus).mockImplementation(() => status("pending", 1, 1));
    vi.mocked(commands.syncInfo).mockImplementation(() =>
      ok({
        registered: true,
        server_url: "https://x.supabase.co",
        registered_at: "2026-11-09T05:00:00.000Z",
        device_code: "T01",
      }),
    );
    vi.mocked(commands.syncDeadOps).mockImplementation(() =>
      ok([
        {
          seq: 41,
          table_name: "batch_items",
          row_id: "bi-1",
          last_error: "COST_LOCKED: batch item cost/qty locked after receive",
          attempts: 1,
          created_at: "2026-11-09T05:00:00.000Z",
        },
      ]),
    );
    vi.mocked(commands.syncResolveDeadOp).mockImplementation(() => ok(null));
    renderAs(["SYNC_MANAGE"]);
    expect(await screen.findByText(/COST_LOCKED/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() =>
      expect(commands.syncResolveDeadOp).toHaveBeenCalledWith({ seq: 41, action: "retry" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Sync now" }));
    await waitFor(() => expect(commands.syncNow).toHaveBeenCalledOnce());
    expect(screen.queryByRole("button", { name: "Register" })).not.toBeInTheDocument();
  });

  it("hides registration and refused changes from staff without SYNC_MANAGE", async () => {
    vi.mocked(commands.syncStatus).mockImplementation(() => status("synced"));
    vi.mocked(commands.syncInfo).mockImplementation(() =>
      ok({ registered: true, server_url: "https://x.supabase.co", registered_at: null, device_code: "T02" }),
    );
    renderAs(["SALE_CREATE"]);
    expect(await screen.findByRole("button", { name: "Sync now" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Register/ })).not.toBeInTheDocument();
    expect(commands.syncDeadOps).not.toHaveBeenCalled();
  });
});
