// i18next set-up (doc §1.8): bundled JSON dictionaries (offline), English source + fallback, Sinhala.
import i18next, { type i18n as I18n, type TFunction } from "i18next";
import { initReactI18next } from "react-i18next";
import type { AppError } from "@/db/bindings";
import { errorKey } from "@/db/client";
import enApp from "./locales/en/app.json";
import enAuth from "./locales/en/auth.json";
import enBatches from "./locales/en/batches.json";
import enCommon from "./locales/en/common.json";
import enErrors from "./locales/en/errors.json";
import enProducts from "./locales/en/products.json";
import siApp from "./locales/si/app.json";
import siAuth from "./locales/si/auth.json";
import siBatches from "./locales/si/batches.json";
import siCommon from "./locales/si/common.json";
import siErrors from "./locales/si/errors.json";
import siProducts from "./locales/si/products.json";

export const LANGUAGES = ["en", "si"] as const;
export type UiLanguage = (typeof LANGUAGES)[number];

export const NAMESPACES = ["common", "errors", "auth", "app", "products", "batches"] as const;

export const resources = {
  en: {
    common: enCommon,
    errors: enErrors,
    auth: enAuth,
    app: enApp,
    products: enProducts,
    batches: enBatches,
  },
  si: {
    common: siCommon,
    errors: siErrors,
    auth: siAuth,
    app: siApp,
    products: siProducts,
    batches: siBatches,
  },
} as const;

export function isUiLanguage(value: string): value is UiLanguage {
  return (LANGUAGES as readonly string[]).includes(value);
}

/** A fresh, synchronously initialised i18next instance (one per app; tests make their own). */
export function createI18n(lng: UiLanguage = "en", extra?: Record<string, Record<string, object>>): I18n {
  const instance = i18next.createInstance();
  void instance.use(initReactI18next).init({
    resources: extra ? { ...resources, ...extra } : resources,
    lng,
    fallbackLng: "en",
    ns: [...NAMESPACES],
    defaultNS: "common",
    interpolation: { escapeValue: false }, // React already escapes
    returnNull: false,
    initAsync: false, // resources are bundled: initialise synchronously
  });
  // Keep <html lang> in step so screen readers and font shaping use the right language (WCAG 3.1.1).
  const syncDocumentLang = (language: string) => {
    if (typeof document !== "undefined") document.documentElement.lang = language;
  };
  syncDocumentLang(instance.language);
  instance.on("languageChanged", syncDocumentLang);
  return instance;
}

/** Translate a Rust `AppError` (Rust never sends display text — doc §1.8). */
export function translateError(t: TFunction, error: AppError): string {
  const key = errorKey(error).replace(/^errors\./, "");
  const kind = key.split(".")[0] ?? "INTERNAL";
  const params: Record<string, unknown> = {};
  if (error.kind === "INVALID_PIN") params.attempts_left = error.attempts_left;
  if (error.kind === "ACCOUNT_LOCKED") params.until = new Date(error.until).toLocaleTimeString();
  const generic = t("INTERNAL", { ns: "errors" });
  const kindFallback = key.includes(".") ? t(`${kind}._`, { ns: "errors", defaultValue: generic }) : generic;
  return t(key, { ns: "errors", ...params, defaultValue: kindFallback });
}
