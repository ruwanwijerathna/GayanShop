import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/db/bindings";
import { err, ok, renderWithProviders } from "@/test/render";
import { FirstRunWizard } from "./FirstRunWizard";

vi.mock("@/db/bindings", () => ({
  commands: {
    firstRunSetup: vi.fn(),
    setupJoinStore: vi.fn(),
    restorePickFile: vi.fn(),
    restoreStageFresh: vi.fn(),
  },
}));

const setup = vi.mocked(commands.firstRunSetup);

beforeEach(() => setup.mockReset());

function fillOwner(pin: string, confirm: string) {
  fireEvent.change(document.querySelector("#owner-name") as HTMLElement, { target: { value: "Gayan" } });
  fireEvent.change(document.querySelector("#owner-pin") as HTMLElement, { target: { value: pin } });
  fireEvent.change(document.querySelector("#owner-pin-confirm") as HTMLElement, {
    target: { value: confirm },
  });
}

describe("FirstRunWizard", () => {
  it("runs in Sinhala and submits the owner-confirmed setup", async () => {
    setup.mockImplementation(() =>
      ok({ store_id: "s", device_id: "d", device_code: "T01", owner_staff_id: "o" }),
    );
    renderWithProviders(<FirstRunWizard />);
    fireEvent.click(screen.getByRole("button", { name: "සිංහල" }));
    expect(screen.getByRole("heading", { name: "මෙම කවුන්ටරය සකසන්න" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "ඉදිරියට" }));
    expect(screen.getByLabelText("වෙළඳසැලේ නම")).toHaveValue("GayanShop");
    expect(screen.getByLabelText("කවුන්ටර කේතය")).toHaveValue("T01");
    fireEvent.click(screen.getByRole("button", { name: "ඉදිරියට" }));
    fillOwner("246810", "246810");
    fireEvent.click(screen.getByRole("button", { name: "සැකසීම අවසන් කරන්න" }));
    await waitFor(() => expect(setup).toHaveBeenCalledOnce());
    expect(setup).toHaveBeenCalledWith({
      store_name: "GayanShop",
      owner_name: "Gayan",
      owner_pin: "246810",
      device_code: "T01",
      ui_language: "si",
    });
  });

  it("catches mismatched PINs before calling Rust", () => {
    renderWithProviders(<FirstRunWizard />);
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fillOwner("246810", "246811");
    fireEvent.click(screen.getByRole("button", { name: "Finish setup" }));
    expect(screen.getByRole("alert")).toHaveTextContent("The two PINs do not match.");
    expect(setup).not.toHaveBeenCalled();
  });

  it("shows Rust validation errors translated", async () => {
    setup.mockImplementation(() => err({ kind: "VALIDATION", code: "PIN_TOO_SHORT", detail: "x" }));
    renderWithProviders(<FirstRunWizard />);
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fillOwner("1234", "1234");
    fireEvent.click(screen.getByRole("button", { name: "Finish setup" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The PIN is too short.");
  });
});

describe("FirstRunWizard → join an existing store", () => {
  it("registers into the owner's store and pulls its history", async () => {
    vi.mocked(commands.setupJoinStore).mockImplementation(() =>
      ok({ pages: 3, rows: 1200, permille: 1000, done: true }),
    );
    renderWithProviders(<FirstRunWizard />);
    fireEvent.click(screen.getByRole("button", { name: /Join an existing store/ }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("heading", { name: "Join an existing store" })).toBeInTheDocument();
    const type = (label: string, value: string) =>
      fireEvent.change(screen.getByLabelText(label), { target: { value } });
    type("Server address", "https://example.supabase.co");
    type("Project key (anon / public)", "anon-key");
    type("Owner's cloud email", "owner@gayanshop.lk");
    type("Owner's cloud password", "secret-1");
    type("Counter name (optional)", "Back counter");
    fireEvent.click(screen.getByRole("button", { name: "Join store" }));
    await waitFor(() => expect(commands.setupJoinStore).toHaveBeenCalledOnce());
    expect(commands.setupJoinStore).toHaveBeenCalledWith({
      register: {
        server_url: "https://example.supabase.co",
        anon_key: "anon-key",
        owner_email: "owner@gayanshop.lk",
        owner_password: "secret-1",
        device_name: "Back counter",
      },
      ui_language: "en",
    });
    expect(setup).not.toHaveBeenCalled();
  });

  it("goes back to the choice", () => {
    renderWithProviders(<FirstRunWizard />);
    fireEvent.click(screen.getByRole("button", { name: /Join an existing store/ }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByRole("heading", { name: "Set up this counter" })).toBeInTheDocument();
  });
});

describe("FirstRunWizard → restore this counter from a backup", () => {
  it("stages the chosen backup file instead of setting up a store", async () => {
    vi.mocked(commands.restorePickFile).mockImplementation(() => ok("D:/usb/GayanShop-T01.db"));
    vi.mocked(commands.restoreStageFresh).mockImplementation(() =>
      ok({
        path: "D:/usb/GayanShop-T01.db",
        store_id: "s",
        device_id: "d",
        device_code: "T01",
        schema_version: 3,
        taken_at: "2026-11-08T05:00:00.000Z",
        size_bytes: 1,
        sha256: "ab",
      }),
    );
    renderWithProviders(<FirstRunWizard />);
    fireEvent.click(screen.getByRole("button", { name: /Restore this counter from a backup/ }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Choose a backup file…" }));
    expect(await screen.findByText("D:/usb/GayanShop-T01.db")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    await waitFor(() => expect(commands.restoreStageFresh).toHaveBeenCalledWith("D:/usb/GayanShop-T01.db"));
    expect(await screen.findByText("The backup is ready. The app is restarting…")).toBeInTheDocument();
    expect(setup).not.toHaveBeenCalled();
  });
});
