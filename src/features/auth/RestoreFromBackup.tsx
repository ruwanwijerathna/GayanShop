import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { CommandError } from "@/db/client";
import { pickRestoreFile, useRestoreStageFresh } from "@/db/hooks";
import { translateError } from "@/i18n";

/**
 * First run on a new PC (or after a reinstall): restore this counter from its backup file instead of setting
 * up a new store. The app restarts, and the Owner finishes the restore with their PIN.
 */
export function RestoreFromBackup({ onBack }: { onBack: () => void }) {
  const { t, i18n } = useTranslation("backup");
  const stage = useRestoreStageFresh();
  const [chosen, setChosen] = useState<string | null>(null);
  const error = stage.error instanceof CommandError ? translateError(i18n.t, stage.error.error) : null;

  const pick = async () => {
    const path = await pickRestoreFile();
    if (path) setChosen(path);
  };

  return (
    <main className="flex min-h-screen items-center justify-center bg-background p-6">
      <section
        className="flex w-full max-w-xl flex-col gap-5 rounded-xl border border-border bg-card p-8"
        aria-labelledby="fresh-restore-title"
      >
        <h1 id="fresh-restore-title" className="text-2xl font-semibold">
          {t("fresh.title")}
        </h1>
        <p className="text-base text-muted-foreground">{t("fresh.intro")}</p>
        <Button type="button" size="pos" variant="outline" className="self-start" onClick={() => void pick()}>
          {t("restore.pickFile")}
        </Button>
        {chosen && <p className="font-mono text-sm break-all">{chosen}</p>}
        {error && (
          <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
            {error}
          </p>
        )}
        {stage.isSuccess && (
          <p role="status" className="rounded-md bg-success-surface p-3 text-success-foreground">
            {t("restore.restarting")}
          </p>
        )}
        <footer className="flex justify-between gap-3">
          <Button type="button" size="pos" variant="outline" disabled={stage.isPending} onClick={onBack}>
            {t("fresh.back")}
          </Button>
          <Button
            type="button"
            size="pos"
            disabled={!chosen || stage.isPending || stage.isSuccess}
            onClick={() => chosen && stage.mutate(chosen)}
          >
            {stage.isPending ? t("restore.staging") : t("fresh.restore")}
          </Button>
        </footer>
      </section>
    </main>
  );
}
