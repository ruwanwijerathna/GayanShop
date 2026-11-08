// Test helpers: render inside the real providers; shape mocked command results like tauri-specta does.
import { render } from "@testing-library/react";
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
