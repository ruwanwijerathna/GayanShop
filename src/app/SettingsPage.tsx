import { useTranslation } from "react-i18next";
import { Can } from "@/components/auth/Can";
import { AppearanceSettings } from "@/components/common/AppearanceSettings";
import { PosTabsList, PosTabsTrigger } from "@/components/common/PosTabs";
import { TouchModeSettings } from "@/components/common/TouchModeSettings";
import { Tabs, TabsContent } from "@/components/ui/tabs";
import { BackupSettings } from "@/features/backup/BackupSettings";
import { RestoreSettings } from "@/features/backup/RestoreSettings";
import { LabelSettings } from "@/features/labels/LabelSettings";
import { PrinterSettings } from "@/features/printing/PrinterSettings";
import { MaintenanceSettings } from "@/features/reports/MaintenanceSettings";
import { SyncSettings } from "@/features/sync/SyncSettings";

/** Settings in tabs: the bill and label printers (the tab a counter visits most), look and touch mode,
 * backup and restore, cloud sync, and maintenance. Composed here, because features never import each other. */
export function SettingsPage() {
  const { t } = useTranslation("app");
  return (
    <section className="mx-auto flex max-w-3xl flex-col gap-6 p-6">
      <h1 className="text-2xl font-semibold">{t("nav.settings")}</h1>
      <Tabs defaultValue="printers" className="gap-6">
        <PosTabsList aria-label={t("settings.tabsLabel")}>
          <PosTabsTrigger value="printers">{t("settings.tabs.printers")}</PosTabsTrigger>
          <PosTabsTrigger value="general">{t("settings.tabs.general")}</PosTabsTrigger>
          <PosTabsTrigger value="backup">{t("settings.tabs.backup")}</PosTabsTrigger>
          <PosTabsTrigger value="sync">{t("settings.tabs.sync")}</PosTabsTrigger>
          <Can perm="SETTINGS_MANAGE">
            <PosTabsTrigger value="maintenance">{t("settings.tabs.maintenance")}</PosTabsTrigger>
          </Can>
        </PosTabsList>
        <TabsContent value="printers" className="flex flex-col gap-6">
          <PrinterSettings />
          <LabelSettings />
        </TabsContent>
        <TabsContent value="general" className="flex flex-col gap-6">
          <AppearanceSettings />
          <TouchModeSettings />
        </TabsContent>
        <TabsContent value="backup" className="flex flex-col gap-6">
          <BackupSettings />
          <Can perm="BACKUP_RESTORE">
            <RestoreSettings />
          </Can>
        </TabsContent>
        <TabsContent value="sync" className="flex flex-col gap-6">
          <SyncSettings />
        </TabsContent>
        <Can perm="SETTINGS_MANAGE">
          <TabsContent value="maintenance" className="flex flex-col gap-6">
            <MaintenanceSettings />
          </TabsContent>
        </Can>
      </Tabs>
    </section>
  );
}
