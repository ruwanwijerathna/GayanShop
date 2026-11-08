import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { createI18n } from "@/i18n";
import { SessionProvider } from "../session";
import { TopBar } from "./TopBar";

function setup(onLanguageChange = vi.fn(), onThemeChange = vi.fn()) {
  const i18n = createI18n("en");
  render(
    <I18nextProvider i18n={i18n}>
      <SessionProvider
        initial={{
          staff_id: "s1",
          display_name: "Gayan",
          role_name: "Owner",
          permissions: [],
          max_discount_bp: 10_000,
          max_override_bp: 10_000,
          ui_language: "en",
        }}
      >
        <TopBar
          storeName="GayanShop"
          deviceCode="T01"
          theme="light"
          onThemeChange={onThemeChange}
          onLanguageChange={onLanguageChange}
        />
      </SessionProvider>
    </I18nextProvider>,
  );
  return { i18n, onLanguageChange, onThemeChange };
}

describe("TopBar", () => {
  it("shows the store, counter and logged-in staff", () => {
    setup();
    expect(screen.getByText("GayanShop")).toBeInTheDocument();
    expect(screen.getByText("T01")).toBeInTheDocument();
    expect(screen.getByText("Gayan · Owner")).toBeInTheDocument();
  });

  it("switches the UI language instantly and reports it", () => {
    const { i18n, onLanguageChange } = setup();
    fireEvent.click(screen.getByRole("button", { name: "සිං" }));
    expect(i18n.resolvedLanguage).toBe("si");
    expect(onLanguageChange).toHaveBeenCalledWith("si");
    expect(screen.getByRole("button", { name: "සිං" })).toHaveAttribute("aria-pressed", "true");
  });

  it("offers the other theme", () => {
    const { onThemeChange } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Theme: Dark" }));
    expect(onThemeChange).toHaveBeenCalledWith("dark");
  });

  it("uses 48 px touch targets for its controls", () => {
    setup();
    for (const button of screen.getAllByRole("button")) {
      expect(button.className).toMatch(/\bh-12\b|\bsize-12\b/);
    }
  });
});
