import { Hand } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useDevicePrefs, useSetDeviceTouchMode } from "@/db/hooks";
import { cn } from "@/lib/utils";

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
        <button
          type="button"
          role="switch"
          aria-checked={on}
          aria-labelledby="touch-label"
          aria-describedby="touch-description"
          disabled={busy}
          onClick={() => set.mutate(!on)}
          className={cn(
            "relative inline-flex h-12 w-20 shrink-0 items-center rounded-full border-2 border-transparent transition-colors focus-visible:ring-[3px] focus-visible:ring-ring focus-visible:outline-none disabled:opacity-50 motion-reduce:transition-none",
            on ? "bg-primary" : "bg-input",
          )}
        >
          <span className="sr-only">{on ? t("touchMode.on") : t("touchMode.off")}</span>
          <span
            aria-hidden
            className={cn(
              "inline-block size-9 rounded-full bg-background shadow transition-transform motion-reduce:transition-none",
              on ? "translate-x-9" : "translate-x-1",
            )}
          />
        </button>
      </div>
    </section>
  );
}
