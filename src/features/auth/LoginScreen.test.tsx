import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/db/bindings";
import { err, ok, renderWithProviders, session } from "@/test/render";
import { LoginScreen } from "./LoginScreen";

vi.mock("@/db/bindings", () => ({ commands: { listLoginStaff: vi.fn(), login: vi.fn() } }));

beforeEach(() => {
  vi.mocked(commands.listLoginStaff).mockImplementation(() =>
    ok([
      { id: "owner", display_name: "Gayan", role_name: "Owner" },
      { id: "cashier", display_name: "Nimal", role_name: "Cashier" },
    ]),
  );
  vi.mocked(commands.login).mockReset();
});

describe("LoginScreen", () => {
  it("logs in with the touch PIN pad", async () => {
    vi.mocked(commands.login).mockImplementation(() => ok(session()));
    renderWithProviders(<LoginScreen storeName="GayanShop" />);
    fireEvent.click(await screen.findByRole("button", { name: /Nimal/ }));
    for (const d of "1111") fireEvent.click(screen.getByRole("button", { name: `Digit ${d}` }));
    expect(screen.getByLabelText("Enter PIN for Nimal")).toHaveValue("1111");
    fireEvent.click(screen.getByRole("button", { name: "Log in" }));
    await waitFor(() => expect(commands.login).toHaveBeenCalledWith("cashier", "1111"));
  });

  it("accepts a physical keyboard (digits only), submits with Enter and shows wrong-PIN feedback", async () => {
    vi.mocked(commands.login).mockImplementation(() => err({ kind: "INVALID_PIN", attempts_left: 4 }));
    renderWithProviders(<LoginScreen storeName="GayanShop" />);
    fireEvent.click(await screen.findByRole("button", { name: /Nimal/ }));
    const field = screen.getByLabelText("Enter PIN for Nimal");
    fireEvent.change(field, { target: { value: "9a9x9 9" } });
    expect(field).toHaveValue("9999");
    fireEvent.submit(field);
    expect(await screen.findByRole("alert")).toHaveTextContent("Incorrect PIN. 4 attempts left.");
    expect(field).toHaveValue("");
  });

  it("explains an idle lock", () => {
    renderWithProviders(<LoginScreen storeName="GayanShop" wasLocked />);
    expect(screen.getByRole("status")).toHaveTextContent("The counter locked after inactivity.");
  });
});
