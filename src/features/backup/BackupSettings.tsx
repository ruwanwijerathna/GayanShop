import { Link } from "@tanstack/react-router";
import { HardDriveDownload, ShieldAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Can } from "@/components/auth/Can";
import { Button } from "@/components/ui/button";
import { CommandError } from "@/db/client";
import { pickBackupFolder, useBackupNow, useBackupStatus } from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";

/** "2 hours ago" in the UI language. */
export function relativeTime(iso: string, now: number, lang: string): string {
  const seconds = Math.round((Date.parse(iso) - now) / 1000);
  const fmt = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });
  const abs = Math.abs(seconds);
  if (abs < 60) return fmt.format(seconds, "second");
  if (abs < 3_600) return fmt.format(Math.round(seconds / 60), "minute");
  if (abs < 86_400) return fmt.format(Math.round(seconds / 3_600), "hour");
  return fmt.format(Math.round(seconds / 86_400), "day");
}

function useElapsed(running: boolean): number {
  const [started, setStarted] = useState<number | null>(null);
  const [now, setNow] = useState(0);
  useEffect(() => {
    if (!running) return;
    const begin = Date.now();
    const kick = window.setTimeout(() => {
      setStarted(begin);
      setNow(begin);
    }, 0);
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => {
      window.clearTimeout(kick);
      window.clearInterval(timer);
    };
  }, [running]);
  return running && started !== null ? Math.floor((now - started) / 1_000) : 0;
}

/**
 * Settings → Backup (doc §3.9): pick a folder (a USB drive), take a verified copy while the till keeps
 * selling, and see when the last one was taken. The copy holds PIN hashes and phone numbers: say so.
 */
export function BackupSettings() {
  const { t, i18n } = useTranslation("backup");
  const status = useBackupStatus();
  const backup = useBackupNow();
  const [choosing, setChoosing] = useState(false);
  const elapsed = useElapsed(backup.isPending);

  const start = async () => {
    setChoosing(true);
    try {
      const dir = await pickBackupFolder();
      if (dir) {
        backup.mutate(dir, {
          onSuccess: () => playCue("complete"),
          onError: () => playCue("error"),
        });
      }
    } finally {
      setChoosing(false);
    }
  };

  const s = status.data;
  const error = backup.error instanceof CommandError ? translateError(i18n.t, backup.error.error) : null;
  return (
    <section className="flex flex-col gap-4 rounded-lg border bg-card p-5" aria-labelledby="backup-title">
      <div className="flex items-center gap-3">
        <HardDriveDownload aria-hidden className="size-6" />
        <h2 id="backup-title" className="text-xl font-semibold">
          {t("title")}
        </h2>
      </div>
      <p className="text-base">
        {s?.last_at
          ? t("last", { when: relativeTime(s.last_at, status.dataUpdatedAt, i18n.resolvedLanguage ?? "en") })
          : t("never")}
      </p>
      {s?.last_path && (
        <p className="font-mono text-sm break-all text-muted-foreground">
          {t("lastPath", { path: s.last_path })}
        </p>
      )}
      <p className="flex items-start gap-2 text-base">
        <ShieldAlert aria-hidden className="mt-0.5 size-5 shrink-0" />
        {t("warning")}
      </p>
      {backup.isSuccess && (
        <div role="status" className="rounded-md bg-success-surface p-3 text-success-foreground">
          <p>{t("done", { path: backup.data.path })}</p>
          <p className="font-mono text-sm break-all">{t("hash", { hash: backup.data.sha256 })}</p>
        </div>
      )}
      {error && (
        <p role="alert" className="text-base text-destructive">
          {error}
        </p>
      )}
      <Can perm="BACKUP_CREATE">
        <Button
          type="button"
          size="pos"
          className="self-start"
          disabled={choosing || backup.isPending}
          onClick={() => void start()}
        >
          {backup.isPending ? t("running", { seconds: elapsed }) : choosing ? t("choosing") : t("button")}
        </Button>
      </Can>
    </section>
  );
}

/** The back-office "Back up today" nudge (doc §3.9): no manual backup in 7 days and no cloud sync yet. */
export function BackupReminder() {
  const { t } = useTranslation("backup");
  const status = useBackupStatus();
  if (!status.data?.reminder_due) return null;
  return (
    <Can perm="BACKUP_CREATE">
      <div
        role="status"
        className="flex flex-wrap items-center gap-3 border-b border-warning bg-warning/15 px-4 py-2 text-base"
      >
        <HardDriveDownload aria-hidden className="size-5 shrink-0" />
        <span className="flex-1">{t("reminder")}</span>
        <Button asChild size="pos" variant="outline">
          <Link to="/settings">{t("reminderAction")}</Link>
        </Button>
      </div>
    </Can>
  );
}
