import { Hand } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useDevicePrefs, useSetDeviceTouchMode } from "@/db/hooks";
import { Switch } from "@/components/ui/switch";

/** Settings → Touch screen: this counter's touch screen mode (local to the counter, never synced). */
export function TouchModeSettings() {
  const { t } = useTranslation("app");
  const prefs = useDevicePrefs();
  const set = useSetDeviceTouchMode();
  const on = prefs.data?.touch_mode ?? false;
  const busy = !prefs.data || set.isPending;

  return (
    <section className="flex flex-col gap-4 rounded-lg border bg-card p-5" aria-labelledby="touch-title">
      <div className="flex items-center gap-3">
        <Hand aria-hidden className="size-6" />
        <h2 id="touch-title" className="text-xl font-semibold">
          {t("touchMode.title")}
        </h2>
      </div>
      <div className="flex items-center justify-between gap-6">
        <div className="flex flex-col gap-1">
          <span id="touch-label" className="text-base font-medium">
            {t("touchMode.label")}
          </span>
          <span id="touch-description" className="text-sm text-muted-foreground">
            {t("touchMode.description")}
          </span>
        </div>
        <Switch
          size="pos"
          checked={on}
          aria-labelledby="touch-label"
          aria-describedby="touch-description"
          disabled={busy}
          onCheckedChange={(v) => set.mutate(v)}
        />
      </div>
    </section>
  );
}
