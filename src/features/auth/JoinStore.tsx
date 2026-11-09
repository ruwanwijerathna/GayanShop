import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { BootstrapProgress, UiLanguage } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useBootstrapProgress, useJoinStore, useResumeBootstrap } from "@/db/hooks";
import { translateError } from "@/i18n";

/** How far the history pull is (doc §6.2), as a progress bar. */
function Progress({ progress }: { progress: BootstrapProgress | null }) {
  const { t } = useTranslation("sync");
  const permille = progress?.permille ?? 0;
  return (
    <div className="flex flex-col gap-2" role="status">
      <p className="text-lg font-semibold">{t("join.progressTitle")}</p>
      <div
        role="progressbar"
        aria-label={t("join.progressTitle")}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.floor(permille / 10)}
        className="h-4 overflow-hidden rounded-full bg-muted"
      >
        <div
          className="h-full bg-primary transition-[width] motion-reduce:transition-none"
          style={{ width: `${permille / 10}%` }}
        />
      </div>
      <p className="text-sm text-muted-foreground">{t("join.progress", { rows: progress?.rows ?? 0 })}</p>
    </div>
  );
}

/**
 * First-run "Join an existing store" (decision #18): the owner signs in with the store's cloud account, this
 * counter gets the next code, and the store's whole history comes in. Staff then log in with their PINs.
 */
export function JoinStore({ language, onBack }: { language: UiLanguage; onBack: () => void }) {
  const { t, i18n } = useTranslation(["sync", "common"]);
  const join = useJoinStore();
  const progress = useBootstrapProgress();
  const [serverUrl, setServerUrl] = useState("");
  const [anonKey, setAnonKey] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [deviceName, setDeviceName] = useState("");
  const error = join.error instanceof CommandError ? translateError(i18n.t, join.error.error) : null;

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    join.mutate(
      {
        register: {
          server_url: serverUrl,
          anon_key: anonKey,
          owner_email: email,
          owner_password: password,
          device_name: deviceName || null,
        },
        ui_language: language,
      },
      { onSettled: () => setPassword("") },
    );
  };
  const field = (id: string, label: string, value: string, set: (v: string) => void, type = "text") => (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        type={type}
        className="h-12 text-base"
        value={value}
        autoComplete="off"
        disabled={join.isPending}
        onChange={(e) => set(e.target.value)}
      />
    </div>
  );

  return (
    <main className="flex min-h-screen items-center justify-center bg-background p-6">
      <form
        className="flex w-full max-w-xl flex-col gap-5 rounded-xl border border-border bg-card p-8"
        onSubmit={submit}
        aria-labelledby="join-title"
      >
        <header>
          <h1 id="join-title" className="text-2xl font-semibold">
            {t("join.title")}
          </h1>
          <p className="text-base text-muted-foreground">{t("join.intro")}</p>
        </header>
        {field("join-url", t("settings.serverUrl"), serverUrl, setServerUrl)}
        {field("join-key", t("settings.anonKey"), anonKey, setAnonKey)}
        {field("join-email", t("settings.ownerEmail"), email, setEmail, "email")}
        {field("join-password", t("settings.ownerPassword"), password, setPassword, "password")}
        {field("join-device-name", t("settings.deviceName"), deviceName, setDeviceName)}
        {join.isPending && <Progress progress={progress} />}
        {error && (
          <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
            {error}
          </p>
        )}
        <footer className="flex justify-between gap-3">
          <Button type="button" size="pos" variant="outline" disabled={join.isPending} onClick={onBack}>
            {t("join.back")}
          </Button>
          <Button type="submit" size="pos" disabled={join.isPending}>
            {t("join.submit")}
          </Button>
        </footer>
      </form>
    </main>
  );
}

/** A joined counter whose history pull was interrupted (doc §6.2): continue it. The till stays closed. */
export function BootstrapResume() {
  const { t, i18n } = useTranslation("sync");
  const resume = useResumeBootstrap();
  const progress = useBootstrapProgress();
  const error = resume.error instanceof CommandError ? translateError(i18n.t, resume.error.error) : null;
  return (
    <main className="flex min-h-screen items-center justify-center bg-background p-6">
      <section
        className="flex w-full max-w-xl flex-col gap-5 rounded-xl border border-border bg-card p-8"
        aria-labelledby="resume-title"
      >
        <h1 id="resume-title" className="text-2xl font-semibold">
          {t("join.resumeTitle")}
        </h1>
        <p className="text-base text-muted-foreground">{t("join.resumeIntro")}</p>
        {resume.isPending && <Progress progress={progress} />}
        {error && (
          <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
            {error}
          </p>
        )}
        <Button type="button" size="pos-lg" disabled={resume.isPending} onClick={() => resume.mutate()}>
          {t("join.resume")}
        </Button>
      </section>
    </main>
  );
}
