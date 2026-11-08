import { useTranslation } from "react-i18next";
import { LanguageToggle } from "@/components/common/LanguageToggle";
import { ThemeToggle, type Theme } from "@/components/common/ThemeToggle";
import type { UiLanguage } from "@/i18n";
import { useSession } from "../session";

export interface TopBarProps {
  storeName: string | null;
  deviceCode: string | null;
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  onLanguageChange?: (lng: UiLanguage) => void;
}

/** 56 px top bar (doc §1.9 layout): store · counter · staff · language · theme. */
export function TopBar({ storeName, deviceCode, theme, onThemeChange, onLanguageChange }: TopBarProps) {
  const { t } = useTranslation();
  const { session } = useSession();
  return (
    <header className="flex h-14 shrink-0 items-center gap-4 border-b border-border bg-card px-4">
      <span className="text-lg font-semibold">{storeName ?? t("app.name")}</span>
      {deviceCode && <span className="rounded border border-input px-2 font-mono text-sm">{deviceCode}</span>}
      <span className="flex-1" />
      {session && (
        <span className="text-sm text-muted-foreground">
          {session.display_name} · {session.role_name}
        </span>
      )}
      <LanguageToggle onChange={onLanguageChange} />
      <ThemeToggle theme={theme} onChange={onThemeChange} />
    </header>
  );
}
