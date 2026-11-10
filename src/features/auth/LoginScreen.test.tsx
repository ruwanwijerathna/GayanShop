import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/db/bindings";
import { err, ok, renderWithProviders, session } from "@/test/render";
import { LoginScreen } from "./LoginScreen";

vi.mock("@/db/bindings", () => ({
  commands: { listLoginStaff: vi.fn(), login: vi.fn(), devicePreferences: vi.fn() },
}));

const OWNER = { id: "owner", display_name: "Gayan", role_name: "Owner" };
const TWO = [OWNER, { id: "cashier", display_name: "Nimal", role_name: "Cashier" }];

function touchMode(on: boolean) {
  vi.mocked(commands.devicePreferences).mockImplementation(() =>
    ok({ default_language: "en", theme: "light", touch_mode: on }),
  );
}

beforeEach(() => {
  vi.mocked(commands.listLoginStaff).mockImplementation(() => ok(TWO));
  vi.mocked(commands.login).mockReset();
  touchMode(false);
});

describe("LoginScreen", () => {
  it("shows the staff picker and the PIN field together; the PIN waits for a name", async () => {
    renderWithProviders(<LoginScreen storeName="GayanShop" />);
    expect(await screen.findByRole("button", { name: /Nimal/ })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByLabelText("PIN")).toBeDisabled();
    expect(screen.getByText("Choose your name, then enter your PIN.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in" })).toBeDisabled();
  });

  it("keyboard mode: compact PIN field, no number pad, Sign in button", async () => {
    vi.mocked(commands.login).mockImplementation(() => ok(session()));
    renderWithProviders(<LoginScreen storeName="GayanShop" />);
    fireEvent.click(await screen.findByRole("button", { name: /Nimal/ }));
    expect(screen.getByRole("button", { name: /Nimal/ })).toHaveAttribute("aria-pressed", "true");
    const field = screen.getByLabelText("Enter PIN for Nimal");
    expect(field).toHaveFocus();
    expect(screen.queryByRole("button", { name: "Digit 1" })).not.toBeInTheDocument();
    fireEvent.change(field, { target: { value: "1111" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    await waitFor(() => expect(commands.login).toHaveBeenCalledWith("cashier", "1111"));
  });

  it("touch mode: signs in with the on-screen number pad", async () => {
    touchMode(true);
    vi.mocked(commands.login).mockImplementation(() => ok(session()));
    renderWithProviders(<LoginScreen storeName="GayanShop" />);
    fireEvent.click(await screen.findByRole("button", { name: /Nimal/ }));
    for (const d of "1111") fireEvent.click(await screen.findByRole("button", { name: `Digit ${d}` }));
    expect(screen.getByLabelText("Enter PIN for Nimal")).toHaveValue("1111");
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    await waitFor(() => expect(commands.login).toHaveBeenCalledWith("cashier", "1111"));
  });

  it("picks a lone staff member automatically", async () => {
    vi.mocked(commands.listLoginStaff).mockImplementation(() => ok([OWNER]));
    renderWithProviders(<LoginScreen storeName="GayanShop" />);
    const field = await screen.findByLabelText("Enter PIN for Gayan");
    expect(field).toBeEnabled();
    expect(screen.getByRole("button", { name: /Gayan/ })).toHaveAttribute("aria-pressed", "true");
  });

  it("digits typed anywhere enter the PIN and Enter signs in", async () => {
    vi.mocked(commands.login).mockImplementation(() => ok(session()));
    renderWithProviders(<LoginScreen storeName="GayanShop" />);
    fireEvent.click(await screen.findByRole("button", { name: /Nimal/ }));
    const field = screen.getByLabelText("Enter PIN for Nimal");
    for (const k of ["4", "3", "2", "x", "1", "Backspace", "1"]) fireEvent.keyDown(document.body, { key: k });
    expect(field).toHaveValue("4321");
    fireEvent.keyDown(document.body, { key: "Enter" });
    await waitFor(() => expect(commands.login).toHaveBeenCalledWith("cashier", "4321"));
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

  it("clears the error and PIN when someone else is chosen", async () => {
    vi.mocked(commands.login).mockImplementation(() => err({ kind: "INVALID_PIN", attempts_left: 4 }));
    renderWithProviders(<LoginScreen storeName="GayanShop" />);
    fireEvent.click(await screen.findByRole("button", { name: /Nimal/ }));
    fireEvent.change(screen.getByLabelText("Enter PIN for Nimal"), { target: { value: "1234" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: /Gayan/ }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Enter PIN for Gayan")).toHaveValue("");
  });

  it("explains an idle lock", () => {
    renderWithProviders(<LoginScreen storeName="GayanShop" wasLocked />);
    expect(screen.getByRole("status")).toHaveTextContent("The counter locked after inactivity.");
  });
});
