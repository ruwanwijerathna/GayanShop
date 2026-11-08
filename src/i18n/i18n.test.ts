import { describe, expect, it } from "vitest";
import { createI18n, isUiLanguage, translateError } from "./index";

describe("i18n", () => {
  it("switches between English and Sinhala instantly", async () => {
    const i18n = createI18n("en");
    expect(i18n.t("actions.cancel")).toBe("Cancel");
    await i18n.changeLanguage("si");
    expect(i18n.t("actions.cancel")).toBe("අවලංගු කරන්න");
  });

  it("falls back to English for a key missing in Sinhala", () => {
    const i18n = createI18n("si", {
      si: { common: { only: "සිංහල" } },
      en: { common: { onlyEn: "English only" } },
    });
    expect(i18n.t("onlyEn")).toBe("English only");
  });

  it("translates Rust errors by code, with parameters", () => {
    const i18n = createI18n("en");
    expect(translateError(i18n.t, { kind: "INVALID_PIN", attempts_left: 3 })).toBe(
      "Incorrect PIN. 3 attempts left.",
    );
    expect(translateError(i18n.t, { kind: "VALIDATION", code: "PIN_TOO_SHORT", detail: "ignored" })).toBe(
      "The PIN is too short.",
    );
    expect(translateError(i18n.t, { kind: "FORBIDDEN", perm: "USER_MANAGE" })).toBe(
      "You don't have permission to do this.",
    );
  });

  it("falls back to the kind's generic message for an unknown code", () => {
    const i18n = createI18n("si");
    expect(translateError(i18n.t, { kind: "VALIDATION", code: "SOMETHING_NEW", detail: "x" })).toBe(
      "කරුණාකර ඇතුළත් කළ දේ පරීක්ෂා කරන්න.",
    );
  });

  it("recognises supported languages", () => {
    expect(isUiLanguage("si")).toBe(true);
    expect(isUiLanguage("fr")).toBe(false);
  });
});
