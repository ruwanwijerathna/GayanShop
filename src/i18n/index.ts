// i18next set-up (doc §1.8): bundled JSON dictionaries (offline), English source + fallback, Sinhala.
import i18next, { type i18n as I18n, type TFunction } from "i18next";
import { initReactI18next } from "react-i18next";
import type { AppError } from "@/db/bindings";
import { errorKey } from "@/db/client";
import enApp from "./locales/en/app.json";
import enAuth from "./locales/en/auth.json";
import enBackup from "./locales/en/backup.json";
import enBatches from "./locales/en/batches.json";
import enCash from "./locales/en/cash.json";
import enCommon from "./locales/en/common.json";
import enErrors from "./locales/en/errors.json";
import enPos from "./locales/en/pos.json";
import enPrinting from "./locales/en/printing.json";
import enLabels from "./locales/en/labels.json";
import enProducts from "./locales/en/products.json";
import enStaff from "./locales/en/staff.json";
import siApp from "./locales/si/app.json";
import siAuth from "./locales/si/auth.json";
import siBackup from "./locales/si/backup.json";
import siBatches from "./locales/si/batches.json";
import siCash from "./locales/si/cash.json";
import siCommon from "./locales/si/common.json";
import siErrors from "./locales/si/errors.json";
import siPos from "./locales/si/pos.json";
import siPrinting from "./locales/si/printing.json";
import siLabels from "./locales/si/labels.json";
import siProducts from "./locales/si/products.json";
import siStaff from "./locales/si/staff.json";

export const LANGUAGES = ["en", "si"] as const;
export type UiLanguage = (typeof LANGUAGES)[number];

export const NAMESPACES = [
  "common",
  "errors",
  "auth",
  "app",
  "products",
  "batches",
  "cash",
  "printing",
  "pos",
  "backup",
  "staff",
  "labels",
] as const;

export const resources = {
  en: {
    common: enCommon,
    errors: enErrors,
    auth: enAuth,
    app: enApp,
    products: enProducts,
    batches: enBatches,
    cash: enCash,
    printing: enPrinting,
    pos: enPos,
    backup: enBackup,
    staff: enStaff,
    labels: enLabels,
  },
  si: {
    common: siCommon,
    errors: siErrors,
    auth: siAuth,
    app: siApp,
    products: siProducts,
    batches: siBatches,
    cash: siCash,
    printing: siPrinting,
    pos: siPos,
    backup: siBackup,
    staff: siStaff,
    labels: siLabels,
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
  if (error.kind === "VALIDATION") params.detail = error.detail;
  const generic = t("INTERNAL", { ns: "errors" });
  const kindFallback = key.includes(".") ? t(`${kind}._`, { ns: "errors", defaultValue: generic }) : generic;
  return t(key, { ns: "errors", ...params, defaultValue: kindFallback });
}
