import { Outlet, useNavigate } from "@tanstack/react-router";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useDbChangedInvalidation } from "@/db/events";
import { BackupReminder } from "@/features/backup/BackupSettings";
import { PrinterBanner } from "@/features/printing/PrinterBanner";
import { useBillScanFallback } from "@/hardware/scanner/ScannerProvider";
import { playCue } from "@/lib/audio";
import { useAppStatus, useLogout, useSetUiLanguage } from "@/db/hooks";
import type { UiLanguage } from "@/i18n";
import { AppSidebar } from "./layouts/AppSidebar";
import { BackOfficeLayout } from "./layouts/BackOfficeLayout";
import { hasPerm } from "@/lib/permissions";
import { useMarkExplicitLock } from "./lock-context";
import { useSession } from "./session";

/** Logged-in shell: grouped sidebar navigation, lock (Ctrl+L) and language toggle (Ctrl+Shift+L). */
export function AppShell() {
  const { i18n } = useTranslation();
  const status = useAppStatus();
  const markExplicitLock = useMarkExplicitLock();
  const logout = useLogout();
  const setLanguage = useSetUiLanguage();
  useDbChangedInvalidation();
  const navigate = useNavigate();

  // A bill barcode scanned anywhere but the till opens that bill on the Bills page.
  const { session } = useSession();
  useBillScanFallback((receipt) => {
    if (!hasPerm(session, "REPORT_SALES") && !hasPerm(session, "REPRINT_RECEIPT")) {
      playCue("error");
      return;
    }
    playCue("tick");
    void navigate({ to: "/orders", search: { receipt, at: Date.now() } });
  });

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
      sidebar={
        <AppSidebar
          storeName={status.data?.store_name ?? null}
          deviceCode={status.data?.device_code ?? null}
          onLock={lock}
        />
      }
    >
      <PrinterBanner />
      <BackupReminder />
      <Outlet />
    </BackOfficeLayout>
  );
}
