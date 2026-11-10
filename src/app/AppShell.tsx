import { Outlet } from "@tanstack/react-router";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useDbChangedInvalidation } from "@/db/events";
import { BackupReminder } from "@/features/backup/BackupSettings";
import { PrinterBanner } from "@/features/printing/PrinterBanner";
import { SyncStatusPill } from "@/features/sync/SyncStatusPill";
import { useAppStatus, useDevicePrefs, useLogout, useSetDeviceTheme, useSetUiLanguage } from "@/db/hooks";
import type { UiLanguage } from "@/i18n";
import { AppSidebar } from "./layouts/AppSidebar";
import { BackOfficeLayout } from "./layouts/BackOfficeLayout";
import { useMarkExplicitLock } from "./lock-context";
import { TopBar } from "./layouts/TopBar";

/** Logged-in shell: top bar, grouped sidebar navigation, lock (Ctrl+L) and language toggle (Ctrl+Shift+L). */
export function AppShell() {
  const { i18n } = useTranslation();
  const status = useAppStatus();
  const prefs = useDevicePrefs();
  const markExplicitLock = useMarkExplicitLock();
  const logout = useLogout();
  const setLanguage = useSetUiLanguage();
  const setTheme = useSetDeviceTheme();
  useDbChangedInvalidation();

  const lock = () => {
    markExplicitLock(); // so the login screen doesn't report this as an idle lock
    logout.mutate();
  };

  const changeLanguage = (lng: UiLanguage) => setLanguage.mutate(lng);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.altKey) return;
      if (e.code === "KeyL" && e.shiftKey) {
        e.preventDefault();
        const next: UiLanguage = i18n.resolvedLanguage === "si" ? "en" : "si";
        void i18n.changeLanguage(next);
        changeLanguage(next);
      } else if (e.code === "KeyL") {
        e.preventDefault();
        lock();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <BackOfficeLayout
      topBar={
        <TopBar
          storeName={status.data?.store_name ?? null}
          deviceCode={status.data?.device_code ?? null}
          theme={prefs.data?.theme ?? "light"}
          onThemeChange={(theme) => setTheme.mutate(theme)}
          onLanguageChange={changeLanguage}
          sync={<SyncStatusPill />}
        />
      }
      sidebar={<AppSidebar deviceCode={status.data?.device_code ?? null} onLock={lock} />}
    >
      <PrinterBanner />
      <BackupReminder />
      <Outlet />
    </BackOfficeLayout>
  );
}
