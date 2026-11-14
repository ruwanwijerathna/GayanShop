import { Moon, Palette, Sun } from "lucide-react";
import { useTranslation } from "react-i18next";
import { SettingsCard } from "@/components/common/SettingsCard";
import { Button } from "@/components/ui/button";
import { LanguageToggle } from "@/components/common/LanguageToggle";
import { useDevicePrefs, useSetDeviceTheme, useSetUiLanguage } from "@/db/hooks";
import type { Theme } from "@/components/common/ThemeToggle";

/** Settings → Appearance & Language: theme (light/dark) and display language (English/Sinhala). */
export function AppearanceSettings() {
  const { t } = useTranslation(["app", "common"]);
  const prefs = useDevicePrefs();
  const setTheme = useSetDeviceTheme();
  const setLanguage = useSetUiLanguage();
  const currentTheme: Theme = prefs.data?.theme ?? "light";
  const busy = !prefs.data || setTheme.isPending;

  return (
    <SettingsCard id="appearance-title" icon={<Palette />} title={t("appearance.title")}>
      <div className="flex flex-col gap-4 divide-y divide-border">
        <div className="flex flex-col justify-between gap-4 pt-1 sm:flex-row sm:items-center">
          <div className="flex flex-col gap-1">
            <span id="theme-label" className="text-base font-medium">
              {t("appearance.themeLabel")}
            </span>
            <span id="theme-description" className="text-sm text-muted-foreground">
              {t("appearance.themeDescription")}
            </span>
          </div>
          <div role="group" aria-labelledby="theme-label" className="inline-flex gap-2">
            <Button
              type="button"
              size="pos"
              variant={currentTheme === "light" ? "default" : "outline"}
              aria-pressed={currentTheme === "light"}
              disabled={busy}
              className="min-w-24 gap-2"
              onClick={() => setTheme.mutate("light")}
            >
              <Sun className="size-4" aria-hidden />
              <span>{t("common:theme.light")}</span>
            </Button>
            <Button
              type="button"
              size="pos"
              variant={currentTheme === "dark" ? "default" : "outline"}
              aria-pressed={currentTheme === "dark"}
              disabled={busy}
              className="min-w-24 gap-2"
              onClick={() => setTheme.mutate("dark")}
            >
              <Moon className="size-4" aria-hidden />
              <span>{t("common:theme.dark")}</span>
            </Button>
          </div>
        </div>

        <div className="flex flex-col justify-between gap-4 pt-4 sm:flex-row sm:items-center">
          <div className="flex flex-col gap-1">
            <span id="language-label" className="text-base font-medium">
              {t("appearance.languageLabel")}
            </span>
            <span id="language-description" className="text-sm text-muted-foreground">
              {t("appearance.languageDescription")}
            </span>
          </div>
          <LanguageToggle onChange={(lng) => setLanguage.mutate(lng)} />
        </div>
      </div>
    </SettingsCard>
  );
}
