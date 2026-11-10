import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "@/db/bindings";
import { ok, renderWithProviders } from "@/test/render";
import { TouchModeSettings } from "./TouchModeSettings";

vi.mock("@/db/bindings", () => ({
  commands: { devicePreferences: vi.fn(), setDeviceTouchMode: vi.fn() },
}));

let touch = false;

beforeEach(() => {
  touch = false;
  vi.mocked(commands.devicePreferences).mockImplementation(() =>
    ok({ default_language: "en", theme: "light", touch_mode: touch }),
  );
  vi.mocked(commands.setDeviceTouchMode).mockImplementation((enabled: boolean) => {
    touch = enabled;
    return ok(null);
  });
});

describe("TouchModeSettings", () => {
  it("is off by default and turns touch screen mode on and back off", async () => {
    renderWithProviders(<TouchModeSettings />);
    const toggle = await screen.findByRole("switch", { name: "Touch screen mode" });
    expect(toggle).toHaveAccessibleDescription(
      "Show on-screen touch keypads and number pads for touch-enabled devices",
    );
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    expect(commands.setDeviceTouchMode).toHaveBeenCalledWith(true);
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
    expect(commands.setDeviceTouchMode).toHaveBeenLastCalledWith(false);
  });
});
