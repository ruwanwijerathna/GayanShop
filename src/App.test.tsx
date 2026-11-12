import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/db/bindings";
import { ok, openTab, renderWithProviders, session } from "@/test/render";
import App from "./App";

vi.mock("@/db/bindings", () => ({
  commands: {
    appStatus: vi.fn(),
    devicePreferences: vi.fn(() => ok({ default_language: "en", theme: "light", touch_mode: false })),
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
    logout: vi.fn(() => ok(null)),
    listProducts: vi.fn(() => ok([])),
    listCategories: vi.fn(() => ok([])),
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
    expect(await screen.findByRole("heading", { name: "GayanShop" })).toBeInTheDocument();
    expect(screen.getByLabelText("Username")).toBeInTheDocument();
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

  it("switches language live from Ctrl+Shift+L while logged in", async () => {
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

    fireEvent.keyDown(window, { key: "L", code: "KeyL", ctrlKey: true, shiftKey: true });
    expect(await screen.findByRole("heading", { name: "කවුන්ටරයේ තත්ත්වය" })).toBeInTheDocument();
    expect(commands.setUiLanguage).toHaveBeenLastCalledWith("si");

    fireEvent.keyDown(window, { key: "L", code: "KeyL", ctrlKey: true, shiftKey: true });
    expect(await screen.findByRole("heading", { name: "Counter status" })).toBeInTheDocument();
    expect(commands.setUiLanguage).toHaveBeenLastCalledWith("en");
  });
});

function loggedIn(overrides: Parameters<typeof session>[0] = {}) {
  status.mockImplementation(() =>
    ok({
      initialized: true,
      store_name: "GayanShop",
      device_code: "T01",
      session: session(overrides),
      bootstrap_pending: false,
      restore_pending: false,
    }),
  );
}

describe("App sidebar", () => {
  it("groups the owner's navigation into Sales, Catalog, Management and System", async () => {
    loggedIn({
      staff_id: "owner",
      display_name: "Gayan",
      role_name: "Owner",
      permissions: [
        "SALE_CREATE",
        "REPORT_SALES",
        "CUSTOMER_VIEW",
        "BATCH_MANAGE",
        "STOCK_ADJUST",
        "QUICK_SALE_PROMOTE",
        "EXPENSE_MANAGE",
        "AUDIT_VIEW",
        "USER_MANAGE",
        "SETTINGS_MANAGE",
      ],
    });
    renderWithProviders(<App />);
    const groups: [string, string[]][] = [
      ["Sales & Counter", ["Counter", "POS Terminal", "Bills", "Cash", "Customers"]],
      ["Catalog & Stock", ["Products & Stock", "Unlisted Items"]],
      ["Management & Reports", ["Reports", "Staff", "Audit log"]],
    ];
    for (const [label, links] of groups) {
      const heading = await screen.findByText(label);
      const group = heading.closest("[data-sidebar=group]");
      expect(group).not.toBeNull();
      const names = within(group as HTMLElement)
        .getAllByRole("link")
        .map((a) => a.getAttribute("aria-label"));
      expect(names).toEqual(links);
    }
    expect(screen.queryByText("System")).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Counter" })).toHaveAttribute("data-active", "true");
    expect(screen.getByRole("link", { name: "Bills" })).toHaveAttribute("data-active", "false");
    expect(screen.getByRole("button", { name: "Gayan" })).toBeInTheDocument();
    expect(screen.getAllByText("T01").length).toBeGreaterThan(0);
  });

  it("hides links, and a whole group, a cashier can't open", async () => {
    loggedIn();
    renderWithProviders(<App />);
    expect(await screen.findByRole("link", { name: "POS Terminal" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Cash" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Products & Stock" })).toBeInTheDocument();
    for (const hidden of ["Bills", "Customers", "Reports", "Audit log", "Staff", "Settings", "Stock"]) {
      expect(screen.queryByRole("link", { name: hidden })).not.toBeInTheDocument();
    }
    expect(screen.queryByText("Management & Reports")).not.toBeInTheDocument();
    expect(screen.queryByText("System")).not.toBeInTheDocument();
  });

  it("collapses to icons and expands again from the header button", async () => {
    loggedIn();
    renderWithProviders(<App />);
    // jsdom's window is 1024 px wide: the 1024 baseline starts collapsed.
    const toggle = await screen.findByRole("button", { name: "Expand sidebar" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(await screen.findByRole("button", { name: "Collapse sidebar" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });

  it("locks from the sidebar footer user menu", async () => {
    loggedIn();
    renderWithProviders(<App />);
    const trigger = await screen.findByRole("button", { name: "Nimal" });
    fireEvent.pointerDown(trigger, { button: 0 });
    fireEvent.click(await screen.findByRole("menuitem", { name: /Lock/ }));
    await waitFor(() => expect(commands.logout).toHaveBeenCalled());
  });

  it("navigates to settings from the sidebar footer user menu and shows appearance settings", async () => {
    loggedIn({
      staff_id: "owner",
      display_name: "Gayan",
      role_name: "Owner",
      permissions: ["SETTINGS_MANAGE"],
    });
    renderWithProviders(<App />);
    const trigger = await screen.findByRole("button", { name: "Gayan" });
    fireEvent.pointerDown(trigger, { button: 0 });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Settings" }));
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Appearance & Language" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "සිං" })).toBeInTheDocument();
  });
});

describe("Products & Stock", () => {
  afterEach(() => {
    window.location.hash = "";
  });

  const OWNER_PERMS = ["SALE_CREATE", "REPORT_FINANCIAL", "PRODUCT_MANAGE"] as const;

  it("opens the catalog tab from the sidebar, with every tab the owner may open", async () => {
    loggedIn({ staff_id: "owner", role_name: "Owner", permissions: [...OWNER_PERMS] });
    renderWithProviders(<App />);
    fireEvent.click(await screen.findByRole("link", { name: "Products & Stock" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Products & Stock" })).toBeInTheDocument();
    const tabs = within(screen.getByRole("tablist", { name: "Products & Stock sections" }));
    expect(tabs.getAllByRole("tab").map((x) => x.textContent)).toEqual([
      "All Products",
      "Stock Adjustments",
      "Stock Valuation",
      "Movement History",
    ]);
    expect(tabs.getByRole("tab", { name: "All Products" })).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByRole("grid", { name: "Products" })).toBeInTheDocument();

    openTab(tabs.getByRole("tab", { name: "Stock Valuation" }));
    await waitFor(() => expect(window.location.hash).toContain("tab=valuation"));
    expect(tabs.getByRole("tab", { name: "Stock Valuation" })).toHaveAttribute("aria-selected", "true");
  });

  it("redirects an old /inventory link to the Stock Adjustments tab", async () => {
    window.location.hash = "#/inventory";
    loggedIn({ staff_id: "owner", role_name: "Owner", permissions: [...OWNER_PERMS] });
    renderWithProviders(<App />);
    const tab = await screen.findByRole("tab", { name: "Stock Adjustments" });
    await waitFor(() => expect(tab).toHaveAttribute("aria-selected", "true"));
    expect(window.location.hash).toContain("#/products");
  });

  it("gives a cashier the catalog and adjustments only; other tabs in the URL fall back", async () => {
    window.location.hash = "#/products?tab=valuation";
    loggedIn();
    renderWithProviders(<App />);
    const tabs = within(await screen.findByRole("tablist", { name: "Products & Stock sections" }));
    expect(tabs.getAllByRole("tab").map((x) => x.textContent)).toEqual(["All Products", "Stock Adjustments"]);
    expect(tabs.getByRole("tab", { name: "All Products" })).toHaveAttribute("aria-selected", "true");
  });
});
