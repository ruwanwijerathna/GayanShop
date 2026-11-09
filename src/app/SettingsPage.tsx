import { useTranslation } from "react-i18next";
import { Can } from "@/components/auth/Can";
import { BackupSettings } from "@/features/backup/BackupSettings";
import { LabelSettings } from "@/features/labels/LabelSettings";
import { PrinterSettings } from "@/features/printing/PrinterSettings";
import { MaintenanceSettings } from "@/features/reports/MaintenanceSettings";
import { SyncSettings } from "@/features/sync/SyncSettings";

/** Settings: this counter's receipt printer and drawer, its label printer and rolls, cloud sync, "Backup Database Now", and maintenance. Composed here, because features
 * never import each other. */
export function SettingsPage() {
  const { t } = useTranslation("app");
  return (
    <section className="mx-auto flex max-w-3xl flex-col gap-6 p-6">
      <h1 className="text-2xl font-semibold">{t("nav.settings")}</h1>
      <PrinterSettings />
      <LabelSettings />
      <SyncSettings />
      <BackupSettings />
      <Can perm="SETTINGS_MANAGE">
        <MaintenanceSettings />
      </Can>
    </section>
  );
}
