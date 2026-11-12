import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/db/bindings";
import { ok, renderWithProviders } from "@/test/render";
import { AppearanceSettings } from "./AppearanceSettings";

vi.mock("@/db/bindings", () => ({
  commands: {
    devicePreferences: vi.fn(),
    setDeviceTheme: vi.fn(),
    setUiLanguage: vi.fn(),
  },
}));

let currentTheme: "light" | "dark" = "light";
let currentLang: "en" | "si" = "en";

beforeEach(() => {
  currentTheme = "light";
  currentLang = "en";
  vi.mocked(commands.devicePreferences).mockImplementation(() =>
    ok({ default_language: currentLang, theme: currentTheme, touch_mode: false }),
  );
  vi.mocked(commands.setDeviceTheme).mockImplementation((theme) => {
    currentTheme = theme as "light" | "dark";
    return ok(null);
  });
  vi.mocked(commands.setUiLanguage).mockImplementation((lang) => {
    currentLang = lang as "en" | "si";
    return ok(null);
  });
});

describe("AppearanceSettings", () => {
  it("renders theme and language controls and switches theme", async () => {
    renderWithProviders(<AppearanceSettings />);
    expect(await screen.findByRole("heading", { name: "Appearance & Language" })).toBeInTheDocument();

    const lightBtn = screen.getByRole("button", { name: /light/i });
    const darkBtn = screen.getByRole("button", { name: /dark/i });

    await waitFor(() => expect(darkBtn).toBeEnabled());
    expect(lightBtn).toHaveAttribute("aria-pressed", "true");
    expect(darkBtn).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(darkBtn);
    await waitFor(() => expect(commands.setDeviceTheme).toHaveBeenCalledWith("dark"));
  });

  it("switches UI language and persists it", async () => {
    renderWithProviders(<AppearanceSettings />);
    const sinhalaBtn = await screen.findByRole("button", { name: "සිං" });
    fireEvent.click(sinhalaBtn);

    await waitFor(() => expect(commands.setUiLanguage).toHaveBeenCalledWith("si"));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "සිං" })).toHaveAttribute("aria-pressed", "true");
    });
  });

  it("uses 48 px touch targets for theme and language buttons", async () => {
    renderWithProviders(<AppearanceSettings />);
    await screen.findByRole("heading", { name: "Appearance & Language" });
    const buttons = screen.getAllByRole("button");
    expect(buttons.length).toBeGreaterThanOrEqual(4);
    for (const button of buttons) {
      expect(button.className).toMatch(/h-12|min-w-14|min-w-24/);
    }
  });
});
