import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/db/bindings";
import { err, ok, renderWithProviders } from "@/test/render";
import { FirstRunWizard } from "./FirstRunWizard";

vi.mock("@/db/bindings", () => ({ commands: { firstRunSetup: vi.fn() } }));

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
