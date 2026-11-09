import { UserRound } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { PinPad } from "@/components/common/PinPad";
import { Button } from "@/components/ui/button";
import type { LoginStaff, RecoveryMode } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useLoginStaff, useRestoreRecover, useRestoreStatus } from "@/db/hooks";
import { translateError } from "@/i18n";

/**
 * Finishing a restore (doc §8 Phase 4). Shown instead of the login screen: until it completes nothing may be
 * written under this counter's identity, a login included. The Owner picks the recovery path and confirms
 * with their PIN, which is checked without writing anything.
 */
export function RestoreRecovery() {
  const { t, i18n } = useTranslation(["backup", "auth"]);
  const lang = i18n.resolvedLanguage ?? "en";
  const status = useRestoreStatus();
  const staff = useLoginStaff();
  const recover = useRestoreRecover();
  const [mode, setMode] = useState<RecoveryMode | null>(null);
  const [owner, setOwner] = useState<LoginStaff | null>(null);
  const [pin, setPin] = useState("");
  const error = recover.error instanceof CommandError ? translateError(i18n.t, recover.error.error) : null;
  const owners = staff.data?.filter((s) => s.role_name === "Owner") ?? [];
  const modes = status.data?.modes ?? [];
  const pending = status.data?.pending;

  const submit = () => {
    if (!mode || !owner) return;
    recover.mutate({ mode, staff_id: owner.id, pin }, { onSettled: () => setPin("") });
  };

  return (
    <main className="flex min-h-screen items-center justify-center bg-background p-6">
      <section
        className="flex w-full max-w-2xl flex-col gap-5 rounded-xl border border-border bg-card p-8"
        aria-labelledby="recovery-title"
      >
        <h1 id="recovery-title" className="text-2xl font-semibold">
          {t("recovery.title")}
        </h1>
        <p className="text-base text-muted-foreground">{t("recovery.intro")}</p>
        {pending?.taken_at && (
          <p className="text-base">
            {t("recovery.takenAt", {
              when: new Intl.DateTimeFormat(lang, { dateStyle: "medium", timeStyle: "short" }).format(
                new Date(pending.taken_at),
              ),
            })}
          </p>
        )}
        <fieldset className="flex flex-col gap-3">
          <legend className="mb-2 text-lg font-semibold">{t("recovery.chooseMode")}</legend>
          {modes.map((m) => (
            <Button
              key={m}
              type="button"
              size="pos-lg"
              variant={mode === m ? "default" : "outline"}
              aria-pressed={mode === m}
              className="h-auto flex-col items-start py-3 text-left"
              onClick={() => setMode(m)}
            >
              <span>{t(`recovery.mode.${m}`)}</span>
              <span className="text-sm font-normal opacity-90">{t(`recovery.modeHint.${m}`)}</span>
            </Button>
          ))}
        </fieldset>
        {mode && !owner && (
          <fieldset className="flex flex-col gap-3">
            <legend className="mb-2 text-lg font-semibold">{t("recovery.owner")}</legend>
            <ul className="grid grid-cols-2 gap-3">
              {owners.map((s) => (
                <li key={s.id}>
                  <Button
                    type="button"
                    size="pos-lg"
                    variant="outline"
                    className="w-full"
                    onClick={() => setOwner(s)}
                  >
                    <UserRound aria-hidden /> {s.display_name}
                  </Button>
                </li>
              ))}
            </ul>
          </fieldset>
        )}
        {mode && owner && (
          <PinPad
            label={t("auth:login.enterPin", { name: owner.display_name })}
            value={pin}
            onChange={setPin}
            onSubmit={submit}
            disabled={recover.isPending}
          />
        )}
        {recover.isPending && (
          <p role="status" className="text-base">
            {t("recovery.working")}
          </p>
        )}
        {error && (
          <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
            {error}
          </p>
        )}
      </section>
    </main>
  );
}
