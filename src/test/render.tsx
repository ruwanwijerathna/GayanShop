// Test helpers: render inside the real providers; shape mocked command results like tauri-specta does.
import { fireEvent, render, screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { Providers } from "@/app/providers";
import type { AppError, SessionInfo } from "@/db/bindings";

export const ok = <T,>(data: T) => Promise.resolve({ status: "ok" as const, data });
export const err = (error: AppError) => Promise.resolve({ status: "error" as const, error });

export function renderWithProviders(ui: ReactElement) {
  return render(<Providers>{ui}</Providers>);
}

export function session(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return {
    staff_id: "cashier",
    display_name: "Nimal",
    role_name: "Cashier",
    permissions: ["SALE_CREATE", "QUICK_SALE"],
    max_discount_bp: 500,
    max_override_bp: 0,
    ui_language: "en",
    ...overrides,
  };
}

/**
 * Pick an option from a shadcn Select (Radix): open it from the keyboard, then choose the option by its
 * visible name, as a keyboard user would.
 */
export function chooseOption(trigger: HTMLElement, name: string | RegExp) {
  fireEvent.keyDown(trigger, { key: "Enter" });
  fireEvent.keyDown(screen.getByRole("option", { name }), { key: "Enter" });
}

/** Switch to a shadcn (Radix) tab: Radix activates on mouse-down / keyboard, not on a synthetic click. */
export function openTab(tab: HTMLElement) {
  fireEvent.mouseDown(tab, { button: 0 });
}

/** The option names a shadcn Select offers: opens it, reads them, and closes it again with Escape. */
export function optionNames(trigger: HTMLElement): string[] {
  fireEvent.keyDown(trigger, { key: "Enter" });
  const names = screen.getAllByRole("option").map((o) => o.textContent ?? "");
  fireEvent.keyDown(screen.getAllByRole("option")[0] ?? trigger, { key: "Escape" });
  return names;
}
