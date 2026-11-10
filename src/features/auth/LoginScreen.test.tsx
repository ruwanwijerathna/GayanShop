import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Providers } from "@/app/providers";
import { commands } from "@/db/bindings";
import { err, ok, renderWithProviders, session } from "@/test/render";
import { LoginScreen } from "./LoginScreen";
import { findStaff, normalizeUsername } from "./username";

vi.mock("@/db/bindings", () => ({
  commands: { listLoginStaff: vi.fn(), login: vi.fn(), devicePreferences: vi.fn() },
}));

const OWNER = { id: "owner", display_name: "Gayan", role_name: "Owner" };
const CASHIER = { id: "cashier", display_name: "Nimal Perera", role_name: "Cashier" };

function touchMode(on: boolean) {
  vi.mocked(commands.devicePreferences).mockImplementation(() =>
    ok({ default_language: "en", theme: "light", touch_mode: on }),
  );
}

async function renderLogin(props: { wasLocked?: boolean } = {}) {
  renderWithProviders(<LoginScreen storeName="GayanShop" {...props} />);
  await waitFor(() => expect(commands.listLoginStaff).toHaveBeenCalled());
  return { user: screen.getByLabelText("Username"), password: screen.getByLabelText("Password") };
}

/** Fill both fields and press Enter in Password (a form submit). */
function signIn(user: HTMLElement, password: HTMLElement, name: string, pin: string) {
  fireEvent.change(user, { target: { value: name } });
  fireEvent.change(password, { target: { value: pin } });
  fireEvent.submit(password);
}

beforeEach(() => {
  vi.mocked(commands.listLoginStaff).mockImplementation(() => ok([OWNER, CASHIER]));
  vi.mocked(commands.login).mockReset();
  touchMode(false);
});

describe("findStaff", () => {
  it("matches a trimmed, case-insensitive username and reports a missing or shared name", () => {
    expect(normalizeUsername("  NIMAL   perera ")).toBe("nimal perera");
    expect(findStaff([OWNER, CASHIER], " nimal PERERA ")).toEqual({ staff: CASHIER });
    expect(findStaff([OWNER, CASHIER], "Kamal")).toEqual({ problem: "notFound" });
    expect(findStaff([OWNER, CASHIER], "   ")).toEqual({ problem: "notFound" });
    expect(findStaff([OWNER, { ...OWNER, id: "owner-2" }], "gayan")).toEqual({ problem: "ambiguous" });
  });
});

describe("LoginScreen", () => {
  it("shows an English username + password form with no staff tiles, username focused", async () => {
    const { user, password } = await renderLogin();
    expect(screen.getByRole("heading", { name: "GayanShop" })).toBeInTheDocument();
    expect(screen.getByText("Sign In to your account")).toBeInTheDocument();
    expect(user).toHaveFocus();
    expect(user).toHaveAttribute("placeholder", "Enter username");
    expect(password).toHaveAttribute("type", "password");
    expect(password).toHaveAttribute("placeholder", "Enter password");
    expect(screen.queryByRole("button", { name: /Nimal/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Digit 1" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign In" })).toBeDisabled();
  });

  it("stays in English when the counter's language is Sinhala", async () => {
    render(
      <Providers language="si">
        <LoginScreen storeName="GayanShop" />
      </Providers>,
    );
    expect(screen.getByLabelText("Username")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign In" })).toBeInTheDocument();
  });

  it("types a username and password and signs in with the button", async () => {
    vi.mocked(commands.login).mockImplementation(() => ok(session()));
    const { user, password } = await renderLogin();
    fireEvent.change(user, { target: { value: "  nimal perera " } });
    fireEvent.change(password, { target: { value: "1111" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign In" }));
    await waitFor(() => expect(commands.login).toHaveBeenCalledWith("cashier", "1111"));
  });

  it("Enter in Username moves to Password; Enter in Password signs in", async () => {
    vi.mocked(commands.login).mockImplementation(() => ok(session()));
    const { user, password } = await renderLogin();
    fireEvent.change(user, { target: { value: "Gayan" } });
    fireEvent.keyDown(user, { key: "Enter" });
    expect(password).toHaveFocus();
    expect(commands.login).not.toHaveBeenCalled();
    fireEvent.change(password, { target: { value: "123456" } });
    fireEvent.submit(password);
    await waitFor(() => expect(commands.login).toHaveBeenCalledWith("owner", "123456"));
  });

  it("prefills a lone staff member's name, still editable", async () => {
    vi.mocked(commands.listLoginStaff).mockImplementation(() => ok([OWNER]));
    const { user } = await renderLogin();
    await waitFor(() => expect(user).toHaveValue("Gayan"));
    fireEvent.change(user, { target: { value: "" } });
    expect(user).toHaveValue("");
  });

  it("an unknown username shows 'User not found' and never calls login", async () => {
    const { user, password } = await renderLogin();
    signIn(user, password, "Kamal", "1111");
    expect(await screen.findByRole("alert")).toHaveTextContent("User not found. Please check your username.");
    expect(commands.login).not.toHaveBeenCalled();
    expect(user).toHaveFocus();
    fireEvent.change(user, { target: { value: "Kama" } });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("a name two staff share is refused rather than guessed", async () => {
    vi.mocked(commands.listLoginStaff).mockImplementation(() => ok([OWNER, { ...OWNER, id: "owner-2" }]));
    const { user, password } = await renderLogin();
    signIn(user, password, "gayan", "1111");
    expect(await screen.findByRole("alert")).toHaveTextContent("More than one staff member has this name.");
    expect(commands.login).not.toHaveBeenCalled();
  });

  it("a wrong password shows an English error, clears the field and keeps focus there", async () => {
    vi.mocked(commands.login).mockImplementation(() => err({ kind: "INVALID_PIN", attempts_left: 4 }));
    const { user, password } = await renderLogin();
    signIn(user, password, "Nimal Perera", "9999");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Incorrect password. Please try again (4 attempts left).",
    );
    await waitFor(() => expect(password).toHaveValue(""));
    expect(password).toHaveFocus();
  });

  it("shows a lockout from Rust in English", async () => {
    vi.mocked(commands.login).mockImplementation(() =>
      err({ kind: "ACCOUNT_LOCKED", until: "2026-11-10T09:05:00.000Z" }),
    );
    const { user, password } = await renderLogin();
    signIn(user, password, "Gayan", "1111");
    expect(await screen.findByRole("alert")).toHaveTextContent(/Too many wrong PINs/);
  });

  it("touch mode: the on-screen number pad types the password", async () => {
    touchMode(true);
    vi.mocked(commands.login).mockImplementation(() => ok(session()));
    const { user, password } = await renderLogin();
    fireEvent.change(user, { target: { value: "Gayan" } });
    for (const d of "2468") fireEvent.click(await screen.findByRole("button", { name: `Digit ${d}` }));
    expect(password).toHaveValue("2468");
    fireEvent.click(screen.getByRole("button", { name: "Sign In" }));
    await waitFor(() => expect(commands.login).toHaveBeenCalledWith("owner", "2468"));
  });

  it("explains an idle lock", async () => {
    await renderLogin({ wasLocked: true });
    expect(screen.getByRole("status")).toHaveTextContent("The counter locked after inactivity.");
  });
});
