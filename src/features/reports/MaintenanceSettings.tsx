// Settings → Maintenance (SETTINGS_MANAGE): "Rebuild reports" recomputes every derived table — rollups,
// customer and quick-sale stats, stock levels and layers, search indexes — from the base tables (doc §5.3).
// Run it after a restore; it never changes a sale, a payment or the stock ledger.
import { Wrench } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { SettingsCard } from "@/components/common/SettingsCard";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { CommandError } from "@/db/client";
import { useRebuildDerived } from "@/db/hooks";
import { translateError } from "@/i18n";

export function MaintenanceSettings() {
  const { t, i18n } = useTranslation(["reports", "common"]);
  const rebuild = useRebuildDerived();
  const [confirm, setConfirm] = useState(false);
  return (
    <SettingsCard id="maintenance-title" icon={<Wrench />} title={t("maintenance.title")}>
      <p className="text-base text-muted-foreground">{t("maintenance.explain")}</p>
      <Button
        type="button"
        size="pos"
        variant="outline"
        className="self-start"
        onClick={() => setConfirm(true)}
      >
        <Wrench aria-hidden />
        {t("maintenance.rebuild")}
      </Button>
      {rebuild.data && (
        <p className="text-sm" aria-live="polite">
          {t("maintenance.done", {
            days: rebuild.data.daily_sales_summary,
            customers: rebuild.data.customer_stats,
            layers: rebuild.data.batch_item_stock,
          })}
        </p>
      )}
      {confirm && (
        <Dialog open onOpenChange={(o) => !o && !rebuild.isPending && setConfirm(false)}>
          <DialogContent className="sm:max-w-lg">
            <DialogHeader>
              <DialogTitle>{t("maintenance.rebuild")}</DialogTitle>
              <DialogDescription>{t("maintenance.confirm")}</DialogDescription>
            </DialogHeader>
            {rebuild.error instanceof CommandError && (
              <p role="alert" className="text-base text-destructive">
                {translateError(i18n.t, rebuild.error.error)}
              </p>
            )}
            <DialogFooter className="gap-2">
              <Button
                type="button"
                size="pos"
                variant="outline"
                disabled={rebuild.isPending}
                onClick={() => setConfirm(false)}
              >
                {t("common:actions.cancel")}
              </Button>
              <Button
                type="button"
                size="pos"
                disabled={rebuild.isPending}
                onClick={() =>
                  rebuild.mutate(undefined, {
                    onSuccess: () => {
                      toast.success(t("maintenance.rebuilt"));
                      setConfirm(false);
                    },
                  })
                }
              >
                {rebuild.isPending ? t("maintenance.running") : t("maintenance.run")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}
    </SettingsCard>
  );
}
