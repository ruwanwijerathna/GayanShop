import { screen } from "@testing-library/react";
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
  },
}));

const status = vi.mocked(commands.appStatus);

describe("App routing", () => {
  it("starts the first-run wizard on a fresh counter", async () => {
    status.mockImplementation(() =>
      ok({ initialized: false, store_name: null, device_code: null, session: null }),
    );
    renderWithProviders(<App />);
    expect(await screen.findByRole("heading", { name: "Set up this counter" })).toBeInTheDocument();
  });

  it("asks for a login when nobody is logged in", async () => {
    status.mockImplementation(() =>
      ok({ initialized: true, store_name: "GayanShop", device_code: "T01", session: null }),
    );
    renderWithProviders(<App />);
    expect(await screen.findByRole("heading", { name: "Who is working?" })).toBeInTheDocument();
  });

  it("hides the audit log from a cashier (Rust still enforces AUDIT_VIEW)", async () => {
    status.mockImplementation(() =>
      ok({ initialized: true, store_name: "GayanShop", device_code: "T01", session: session() }),
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
});
