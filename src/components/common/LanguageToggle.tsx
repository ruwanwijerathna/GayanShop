import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { LANGUAGES, type UiLanguage } from "@/i18n";
import { cn } from "@/lib/utils";

const SHORT: Record<UiLanguage, string> = { en: "EN", si: "සිං" };

/** EN | සිං toggle (Ctrl+Shift+L). Switches instantly; `onChange` persists the choice (staff/device). */
export function LanguageToggle({ onChange }: { onChange?: (lng: UiLanguage) => void }) {
  const { i18n, t } = useTranslation();
  return (
    <div role="group" aria-label={t("language.label")} className="inline-flex gap-1">
      {LANGUAGES.map((lng) => {
        const active = i18n.resolvedLanguage === lng;
        return (
          <Button
            key={lng}
            type="button"
            size="pos"
            variant={active ? "default" : "outline"}
            aria-pressed={active}
            lang={lng}
            title={t(`language.${lng}`)}
            className={cn("min-w-14")}
            onClick={() => {
              void i18n.changeLanguage(lng);
              onChange?.(lng);
            }}
          >
            {SHORT[lng]}
          </Button>
        );
      })}
    </div>
  );
}
