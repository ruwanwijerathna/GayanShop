import { Moon, Sun } from "lucide-react";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { WithTooltip } from "@/components/common/WithTooltip";

export type Theme = "light" | "dark";

/** Apply the theme to <html> (doc §1.9: `.dark` class; light is the counter default). */
export function useApplyTheme(theme: Theme) {
  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
  }, [theme]);
}

export function ThemeToggle({ theme, onChange }: { theme: Theme; onChange: (theme: Theme) => void }) {
  const { t } = useTranslation();
  const next: Theme = theme === "dark" ? "light" : "dark";
  return (
    <WithTooltip label={`${t("theme.label")}: ${t(`theme.${next}`)}`}>
      <Button
        type="button"
        size="icon-pos"
        variant="outline"
        aria-label={`${t("theme.label")}: ${t(`theme.${next}`)}`}
        onClick={() => onChange(next)}
      >
        {theme === "dark" ? <Sun aria-hidden /> : <Moon aria-hidden />}
      </Button>
    </WithTooltip>
  );
}
