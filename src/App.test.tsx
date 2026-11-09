import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { commands } from "@/db/bindings";
import { ok, renderWithProviders, session } from "@/test/render";
import App from "./App";

vi.mock("@/db/bindings", () => ({
  commands: {
    appStatus: vi.fn(),
    devicePreferences: vi.fn(() => ok({ default_language: "en", theme: "light" })),
    listLoginStaff: vi.fn(() => ok([])),
    appInfo: vi.fn(() => ok({ app_version: "0.1.0", schema_version: 1, data_dir: "C:\\data" })),
    integrityCheck: vi.fn(() =>
      ok({ quick_check: "ok", pending_orders: 0, unreversed_voids: 0, stale_sync_flag: false }),
    ),
    auditPage: vi.fn(() => ok({ rows: [], next_cursor: null })),
    setUiLanguage: vi.fn(() => ok(null)),
    syncStatus: vi.fn(() =>
      ok({ state: "synced", pending: 0, dead: 0, last_success_at: null, last_error: null, chain_broken: [] }),
    ),
    syncBootstrap: vi.fn(() => ok({ pages: 2, rows: 700, permille: 1000, done: true })),
    restoreStatus: vi.fn(() => ok({ pending: null, modes: ["local"], device_code: "T01" })),
  },
}));

const status = vi.mocked(commands.appStatus);

describe("App routing", () => {
  it("shows the restore recovery instead of the login while a restore is pending", async () => {
    status.mockImplementation(() =>
      ok({
        initialized: true,
        store_name: "GayanShop",
        device_code: "T01",
        session: null,
        bootstrap_pending: false,
        restore_pending: true,
      }),
    );
    renderWithProviders(<App />);
    expect(await screen.findByRole("heading", { name: "Finish the restore" })).toBeInTheDocument();
  });

  it("offers to finish an interrupted history pull before anyone can log in", async () => {
    status.mockImplementation(() =>
      ok({
        initialized: true,
        store_name: "GayanShop",
        device_code: "T02",
        session: null,
        bootstrap_pending: true,
        restore_pending: false,
      }),
    );
    renderWithProviders(<App />);
    expect(await screen.findByRole("heading", { name: "Finishing setup" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(commands.syncBootstrap).toHaveBeenCalledOnce());
  });

  it("starts the first-run wizard on a fresh counter", async () => {
    status.mockImplementation(() =>
      ok({
        initialized: false,
        store_name: null,
        device_code: null,
        session: null,
        bootstrap_pending: false,
        restore_pending: false,
      }),
    );
    renderWithProviders(<App />);
    expect(await screen.findByRole("heading", { name: "Set up this counter" })).toBeInTheDocument();
  });

  it("lets the wizard switch to Sinhala (the device default must not override it)", async () => {
    status.mockImplementation(() =>
      ok({
        initialized: false,
        store_name: null,
        device_code: null,
        session: null,
        bootstrap_pending: false,
        restore_pending: false,
      }),
    );
    renderWithProviders(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "සිංහල" }));
    expect(await screen.findByRole("button", { name: "ඉදිරියට" })).toBeInTheDocument();
  });

  it("asks for a login when nobody is logged in", async () => {
    status.mockImplementation(() =>
      ok({
        initialized: true,
        store_name: "GayanShop",
        device_code: "T01",
        session: null,
        bootstrap_pending: false,
        restore_pending: false,
      }),
    );
    renderWithProviders(<App />);
    expect(await screen.findByRole("heading", { name: "Who is working?" })).toBeInTheDocument();
  });

  it("hides the audit log from a cashier (Rust still enforces AUDIT_VIEW)", async () => {
    status.mockImplementation(() =>
      ok({
        initialized: true,
        store_name: "GayanShop",
        device_code: "T01",
        session: session(),
        bootstrap_pending: false,
        restore_pending: false,
      }),
    );
    renderWithProviders(<App />);
    expect(await screen.findByRole("heading", { name: "Counter status" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /Audit log/ })).not.toBeInTheDocument();
    expect(await screen.findByText("Healthy")).toBeInTheDocument();
  });

  it("shows the audit log to the owner and applies the owner's language", async () => {
    status.mockImplementation(() =>
      ok({
        initialized: true,
        bootstrap_pending: false,
        restore_pending: false,
        store_name: "GayanShop",
        device_code: "T01",
        session: session({
          staff_id: "owner",
          role_name: "Owner",
          permissions: ["AUDIT_VIEW"],
          ui_language: "si",
        }),
      }),
    );
    renderWithProviders(<App />);
    expect(await screen.findByRole("link", { name: /විගණන සටහන/ })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "කවුන්ටරයේ තත්ත්වය" })).toBeInTheDocument();
  });

  it("switches language live from the toggle and Ctrl+Shift+L while logged in", async () => {
    status.mockImplementation(() =>
      ok({
        initialized: true,
        store_name: "GayanShop",
        device_code: "T01",
        session: session(),
        bootstrap_pending: false,
        restore_pending: false,
      }),
    );
    renderWithProviders(<App />);
    expect(await screen.findByRole("heading", { name: "Counter status" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "සිං" }));
    expect(await screen.findByRole("heading", { name: "කවුන්ටරයේ තත්ත්වය" })).toBeInTheDocument();
    expect(commands.setUiLanguage).toHaveBeenLastCalledWith("si");

    fireEvent.keyDown(window, { key: "L", code: "KeyL", ctrlKey: true, shiftKey: true });
    expect(await screen.findByRole("heading", { name: "Counter status" })).toBeInTheDocument();
    expect(commands.setUiLanguage).toHaveBeenLastCalledWith("en");
  });
});
