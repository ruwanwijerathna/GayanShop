import { Link, Outlet } from "@tanstack/react-router";
import {
  Banknote,
  Boxes,
  History,
  Lock,
  Package,
  Settings,
  ShoppingCart,
  Store,
  Users,
  Warehouse,
  Zap,
  Contact,
  Receipt,
  BarChart3,
  ScrollText,
} from "lucide-react";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Can } from "@/components/auth/Can";
import { Kbd } from "@/components/common/Kbd";
import { Button } from "@/components/ui/button";
import { useDbChangedInvalidation } from "@/db/events";
import { BackupReminder } from "@/features/backup/BackupSettings";
import { PrinterBanner } from "@/features/printing/PrinterBanner";
import { useAppStatus, useDevicePrefs, useLogout, useSetDeviceTheme, useSetUiLanguage } from "@/db/hooks";
import type { UiLanguage } from "@/i18n";
import { BackOfficeLayout } from "./layouts/BackOfficeLayout";
import { useMarkExplicitLock } from "./lock-context";
import { TopBar } from "./layouts/TopBar";

const NAV_LINK =
  "flex h-12 items-center gap-3 rounded-md px-3 text-base hover:bg-accent [&.active]:bg-primary [&.active]:text-primary-foreground";

/** Logged-in shell: top bar, navigation, lock (Ctrl+L) and language toggle (Ctrl+Shift+L). */
export function AppShell() {
  const { t, i18n } = useTranslation(["app", "auth"]);
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
        />
      }
      nav={
        <>
          <Link to="/" className={NAV_LINK} aria-label={t("nav.home")}>
            <Store aria-hidden className="size-5 shrink-0" />
            <span className="hidden xl:inline">{t("nav.home")}</span>
          </Link>
          <Can perm="SALE_CREATE">
            <Link to="/pos" className={NAV_LINK} aria-label={t("nav.pos")}>
              <ShoppingCart aria-hidden className="size-5 shrink-0" />
              <span className="hidden xl:inline">{t("nav.pos")}</span>
            </Link>
          </Can>
          <Link to="/cash" className={NAV_LINK} aria-label={t("nav.cash")}>
            <Banknote aria-hidden className="size-5 shrink-0" />
            <span className="hidden xl:inline">{t("nav.cash")}</span>
          </Link>
          <Link to="/products" className={NAV_LINK} aria-label={t("nav.products")}>
            <Package aria-hidden className="size-5 shrink-0" />
            <span className="hidden xl:inline">{t("nav.products")}</span>
          </Link>
          <Can perm="BATCH_MANAGE">
            <Link to="/batches" className={NAV_LINK} aria-label={t("nav.batches")}>
              <Boxes aria-hidden className="size-5 shrink-0" />
              <span className="hidden xl:inline">{t("nav.batches")}</span>
            </Link>
          </Can>
          <Can perm={["STOCK_DAMAGE", "STOCK_ADJUST", "REPORT_FINANCIAL"]}>
            <Link to="/inventory" className={NAV_LINK} aria-label={t("nav.inventory")}>
              <Warehouse aria-hidden className="size-5 shrink-0" />
              <span className="hidden xl:inline">{t("nav.inventory")}</span>
            </Link>
          </Can>
          <Can perm="QUICK_SALE_PROMOTE">
            <Link to="/quick-sales" className={NAV_LINK} aria-label={t("nav.quickSales")}>
              <Zap aria-hidden className="size-5 shrink-0" />
              <span className="hidden xl:inline">{t("nav.quickSales")}</span>
            </Link>
          </Can>
          <Can perm="REPORT_SALES">
            <Link to="/reports" className={NAV_LINK} aria-label={t("nav.reports")}>
              <BarChart3 aria-hidden className="size-5 shrink-0" />
              <span className="hidden xl:inline">{t("nav.reports")}</span>
            </Link>
          </Can>
          <Can perm="REPORT_SALES">
            <Link to="/orders" className={NAV_LINK} aria-label={t("nav.orders")}>
              <ScrollText aria-hidden className="size-5 shrink-0" />
              <span className="hidden xl:inline">{t("nav.orders")}</span>
            </Link>
          </Can>
          <Can perm="CUSTOMER_VIEW">
            <Link to="/customers" className={NAV_LINK} aria-label={t("nav.customers")}>
              <Contact aria-hidden className="size-5 shrink-0" />
              <span className="hidden xl:inline">{t("nav.customers")}</span>
            </Link>
          </Can>
          <Can perm="EXPENSE_MANAGE">
            <Link to="/expenses" className={NAV_LINK} aria-label={t("nav.expenses")}>
              <Receipt aria-hidden className="size-5 shrink-0" />
              <span className="hidden xl:inline">{t("nav.expenses")}</span>
            </Link>
          </Can>
          <Can perm="AUDIT_VIEW">
            <Link to="/audit" className={NAV_LINK} aria-label={t("nav.audit")}>
              <History aria-hidden className="size-5 shrink-0" />
              <span className="hidden xl:inline">{t("nav.audit")}</span>
            </Link>
          </Can>
          <Can perm="USER_MANAGE">
            <Link to="/staff" className={NAV_LINK} aria-label={t("nav.staff")}>
              <Users aria-hidden className="size-5 shrink-0" />
              <span className="hidden xl:inline">{t("nav.staff")}</span>
            </Link>
          </Can>
          <Can perm={["SETTINGS_MANAGE", "BACKUP_CREATE"]}>
            <Link to="/settings" className={NAV_LINK} aria-label={t("nav.settings")}>
              <Settings aria-hidden className="size-5 shrink-0" />
              <span className="hidden xl:inline">{t("nav.settings")}</span>
            </Link>
          </Can>
          <span className="flex-1" />
          <Button type="button" size="pos" variant="outline" onClick={lock} className="justify-start">
            <Lock aria-hidden />
            <span className="hidden xl:inline">{t("auth:lock")}</span>
            <Kbd className="hidden xl:inline-flex">{t("shortcuts.lock")}</Kbd>
          </Button>
        </>
      }
    >
      <PrinterBanner />
      <BackupReminder />
      <Outlet />
    </BackOfficeLayout>
  );
}
