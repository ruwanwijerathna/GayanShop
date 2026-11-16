import { CloudCog, HardDriveDownload, Printer, SlidersHorizontal, Wrench } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Can } from "@/components/auth/Can";
import { AppearanceSettings } from "@/components/common/AppearanceSettings";
import { TouchModeSettings } from "@/components/common/TouchModeSettings";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { BackupSettings } from "@/features/backup/BackupSettings";
import { RestoreSettings } from "@/features/backup/RestoreSettings";
import { LabelSettings } from "@/features/labels/LabelSettings";
import { PrinterSettings } from "@/features/printing/PrinterSettings";
import { MaintenanceSettings } from "@/features/reports/MaintenanceSettings";
import { SyncSettings } from "@/features/sync/SyncSettings";

const TRIGGER = "h-full min-w-32 rounded-lg px-4 text-sm";
const PANEL = "flex max-w-3xl flex-col gap-6 motion-safe:animate-in motion-safe:fade-in-0";

/** Settings, laid out like Staff and Products: a title with a one-line subtitle, then tabs: the bill and
 * label printers (the tab a counter visits most), look and touch mode, backup and restore, cloud sync, and
 * maintenance. Composed here, because features never import each other. */
export function SettingsPage() {
  const { t } = useTranslation("app");
  return (
    <section className="flex w-full flex-1 flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">{t("nav.settings")}</h1>
        <p className="text-sm text-muted-foreground">{t("settings.subtitle")}</p>
      </div>
      <Tabs defaultValue="printers" className="gap-5">
        <TabsList
          aria-label={t("settings.tabsLabel")}
          className="h-12 max-w-full self-start overflow-x-auto rounded-xl p-1"
        >
          <TabsTrigger value="printers" className={TRIGGER}>
            <Printer aria-hidden />
            {t("settings.tabs.printers")}
          </TabsTrigger>
          <TabsTrigger value="general" className={TRIGGER}>
            <SlidersHorizontal aria-hidden />
            {t("settings.tabs.general")}
          </TabsTrigger>
          <TabsTrigger value="backup" className={TRIGGER}>
            <HardDriveDownload aria-hidden />
            {t("settings.tabs.backup")}
          </TabsTrigger>
          <TabsTrigger value="sync" className={TRIGGER}>
            <CloudCog aria-hidden />
            {t("settings.tabs.sync")}
          </TabsTrigger>
          <Can perm="SETTINGS_MANAGE">
            <TabsTrigger value="maintenance" className={TRIGGER}>
              <Wrench aria-hidden />
              {t("settings.tabs.maintenance")}
            </TabsTrigger>
          </Can>
        </TabsList>
        <TabsContent
          value="printers"
          className="flex flex-col gap-6 motion-safe:animate-in motion-safe:fade-in-0"
        >
          <PrinterSettings />
          <LabelSettings />
        </TabsContent>
        <TabsContent value="general" className={PANEL}>
          <AppearanceSettings />
          <TouchModeSettings />
        </TabsContent>
        <TabsContent value="backup" className={PANEL}>
          <BackupSettings />
          <Can perm="BACKUP_RESTORE">
            <RestoreSettings />
          </Can>
        </TabsContent>
        <TabsContent value="sync" className={PANEL}>
          <SyncSettings />
        </TabsContent>
        <Can perm="SETTINGS_MANAGE">
          <TabsContent value="maintenance" className={PANEL}>
            <MaintenanceSettings />
          </TabsContent>
        </Can>
      </Tabs>
    </section>
  );
}
