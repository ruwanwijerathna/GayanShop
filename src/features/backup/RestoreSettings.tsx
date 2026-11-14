import { History, TriangleAlert } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingsCard } from "@/components/common/SettingsCard";
import { Button } from "@/components/ui/button";
import { CommandError } from "@/db/client";
import { pickRestoreFile, useRestoreCandidates, useRestoreStage } from "@/db/hooks";
import { translateError } from "@/i18n";

function when(iso: string, lang: string): string {
  return new Intl.DateTimeFormat(lang, { dateStyle: "medium", timeStyle: "short" }).format(new Date(iso));
}

function megabytes(bytes: number, lang: string): string {
  return new Intl.NumberFormat(lang, { maximumFractionDigits: 1 }).format(bytes / 1_048_576);
}

/**
 * Settings → Backup → Restore (Owner only, `BACKUP_RESTORE`; doc §8 Phase 4). Choose an automatic or manual
 * backup, or any backup file. The current database is backed up first, the app restarts, and the Owner
 * finishes the restore before the till reopens.
 */
export function RestoreSettings() {
  const { t, i18n } = useTranslation("backup");
  const lang = i18n.resolvedLanguage ?? "en";
  const candidates = useRestoreCandidates();
  const stage = useRestoreStage();
  const [chosen, setChosen] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  const error = stage.error instanceof CommandError ? translateError(i18n.t, stage.error.error) : null;

  const pick = async () => {
    setPicking(true);
    try {
      const path = await pickRestoreFile();
      if (path) setChosen(path);
    } finally {
      setPicking(false);
    }
  };

  return (
    <SettingsCard id="restore-title" icon={<History />} title={t("restore.title")}>
      <p className="text-base text-muted-foreground">{t("restore.intro")}</p>
      {candidates.data && candidates.data.length > 0 ? (
        <ul className="flex max-h-72 flex-col gap-2 overflow-y-auto">
          {candidates.data.map((c) => (
            <li key={c.path}>
              <Button
                type="button"
                size="pos"
                variant={chosen === c.path ? "default" : "outline"}
                aria-pressed={chosen === c.path}
                className="h-auto w-full justify-between py-2 text-left"
                onClick={() => setChosen(c.path)}
              >
                <span>
                  {t(`restore.kind.${c.kind}`)} · {when(c.taken_at, lang)}
                </span>
                <span className="text-sm font-normal">
                  {t("restore.size", { mb: megabytes(c.size_bytes, lang) })}
                </span>
              </Button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-base">{t("restore.none")}</p>
      )}
      <Button
        type="button"
        size="pos"
        variant="outline"
        className="self-start"
        disabled={picking}
        onClick={() => void pick()}
      >
        {t("restore.pickFile")}
      </Button>
      {chosen && (
        <div
          className="flex flex-col gap-3 rounded-md border border-destructive p-4"
          role="group"
          aria-label={t("restore.confirmTitle")}
        >
          <p className="flex items-start gap-2 text-base font-semibold">
            <TriangleAlert aria-hidden className="mt-0.5 size-5 shrink-0" />
            {t("restore.confirmTitle")}
          </p>
          <p className="font-mono text-sm break-all">{chosen}</p>
          <p className="text-base">{t("restore.confirmBody")}</p>
          {error && (
            <p role="alert" className="text-base text-destructive">
              {error}
            </p>
          )}
          {stage.isSuccess ? (
            <p role="status" className="rounded-md bg-success-surface p-3 text-success-foreground">
              {t("restore.restarting")}
            </p>
          ) : (
            <div className="flex flex-wrap gap-3">
              <Button
                type="button"
                size="pos"
                variant="destructive"
                disabled={stage.isPending}
                onClick={() => stage.mutate(chosen)}
              >
                {stage.isPending ? t("restore.staging") : t("restore.confirm")}
              </Button>
              <Button
                type="button"
                size="pos"
                variant="outline"
                disabled={stage.isPending}
                onClick={() => setChosen(null)}
              >
                {t("restore.cancel")}
              </Button>
            </div>
          )}
        </div>
      )}
    </SettingsCard>
  );
}
