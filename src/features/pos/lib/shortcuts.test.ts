import { describe, expect, it } from "vitest";
import { routeShortcut, type ShortcutContext } from "./shortcuts";

const key = (code: string, mods: Partial<{ ctrlKey: boolean; altKey: boolean; metaKey: boolean }> = {}) => ({
  code,
  ctrlKey: false,
  altKey: false,
  metaKey: false,
  ...mods,
});
const IDLE: ShortcutContext = { capturing: false, dialogOpen: false, inTextField: false };

describe("routeShortcut", () => {
  it.each([
    ["F1", "help"],
    ["F2", "quickSale"],
    ["F3", "search"],
    ["F4", "phone"],
    ["F6", "lineDiscount"],
    ["F7", "billDiscount"],
    ["F8", "hold"],
    ["F9", "pay"],
    ["F10", "void"],
    ["F11", "recall"],
    ["Escape", "cancel"],
    ["NumpadAdd", "pay"],
    ["NumpadDivide", "phone"],
  ])("%s → %s", (code, action) => {
    expect(routeShortcut(key(code), IDLE)).toBe(action);
  });

  it("is silent while the scanner is capturing", () => {
    expect(routeShortcut(key("F9"), { ...IDLE, capturing: true })).toBeNull();
    expect(routeShortcut(key("Escape"), { ...IDLE, capturing: true })).toBeNull();
  });

  it("leaves the keyboard to an open dialog", () => {
    expect(routeShortcut(key("F9"), { ...IDLE, dialogOpen: true })).toBeNull();
  });

  it("only lets F-keys and Esc through in a text field", () => {
    const typing = { ...IDLE, inTextField: true };
    expect(routeShortcut(key("F2"), typing)).toBe("quickSale");
    expect(routeShortcut(key("Escape"), typing)).toBe("cancel");
    expect(routeShortcut(key("NumpadAdd"), typing)).toBeNull();
    expect(routeShortcut(key("KeyA"), typing)).toBeNull();
  });

  it("ignores plain keys and modifier chords (Ctrl+L belongs to the shell)", () => {
    expect(routeShortcut(key("KeyA"), IDLE)).toBeNull();
    expect(routeShortcut(key("F5"), IDLE)).toBeNull();
    expect(routeShortcut(key("F9", { ctrlKey: true }), IDLE)).toBeNull();
    expect(routeShortcut(key("KeyL", { ctrlKey: true }), IDLE)).toBeNull();
  });
});
