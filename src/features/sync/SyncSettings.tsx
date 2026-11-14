import { CloudCog } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Can } from "@/components/auth/Can";
import { SettingsCard } from "@/components/common/SettingsCard";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { CommandError } from "@/db/client";
import {
  useResolveDeadOp,
  useSyncDeadOps,
  useSyncInfo,
  useSyncLogs,
  useSyncNow,
  useSyncRegister,
  useSyncStatus,
} from "@/db/hooks";
import { translateError } from "@/i18n";

function when(iso: string, lang: string): string {
  return new Intl.DateTimeFormat(lang, { dateStyle: "medium", timeStyle: "short" }).format(new Date(iso));
}

/** Register (or re-register) this counter with the owner's cloud account (`SYNC_MANAGE`, doc §6.5). */
function RegisterForm({ again, defaultUrl }: { again: boolean; defaultUrl: string }) {
  const { t, i18n } = useTranslation("sync");
  const register = useSyncRegister();
  const [serverUrl, setServerUrl] = useState(defaultUrl);
  const [anonKey, setAnonKey] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [deviceName, setDeviceName] = useState("");
  const error = register.error instanceof CommandError ? translateError(i18n.t, register.error.error) : null;
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    register.mutate(
      {
        server_url: serverUrl,
        anon_key: anonKey,
        owner_email: email,
        owner_password: password,
        device_name: deviceName || null,
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
        onChange={(e) => set(e.target.value)}
      />
    </div>
  );
  return (
    <form className="flex flex-col gap-4" onSubmit={submit} aria-labelledby="sync-register-title">
      <h3 id="sync-register-title" className="text-lg font-semibold">
        {again ? t("settings.reregisterTitle") : t("settings.registerTitle")}
      </h3>
      <p className="text-base text-muted-foreground">{t("settings.registerIntro")}</p>
      {field("sync-url", t("settings.serverUrl"), serverUrl, setServerUrl)}
      {field("sync-key", t("settings.anonKey"), anonKey, setAnonKey)}
      {field("sync-email", t("settings.ownerEmail"), email, setEmail, "email")}
      {field("sync-password", t("settings.ownerPassword"), password, setPassword, "password")}
      {field("sync-device-name", t("settings.deviceName"), deviceName, setDeviceName)}
      {error && (
        <p role="alert" className="text-base text-destructive">
          {error}
        </p>
      )}
      {register.isSuccess && (
        <p role="status" className="rounded-md bg-success-surface p-3 text-success-foreground">
          {t("settings.registered", { code: register.data.device_code })}
        </p>
      )}
      <Button type="submit" size="pos" className="self-start" disabled={register.isPending}>
        {register.isPending ? t("settings.registering") : t("settings.register")}
      </Button>
    </form>
  );
}

/** The ops the cloud refused: retry (sent again as the row is now) or discard. Both are audited. */
function DeadOps() {
  const { t } = useTranslation("sync");
  const dead = useSyncDeadOps();
  const resolve = useResolveDeadOp();
  if (!dead.data?.length) return null;
  return (
    <section className="flex flex-col gap-3" aria-labelledby="sync-dead-title">
      <h3 id="sync-dead-title" className="text-lg font-semibold">
        {t("settings.deadTitle")}
      </h3>
      <p className="text-base text-muted-foreground">{t("settings.deadIntro")}</p>
      <ul className="flex flex-col gap-2">
        {dead.data.map((op) => (
          <li key={op.seq} className="flex flex-wrap items-center gap-3 rounded-md border p-3">
            <span className="flex-1">
              <span className="block font-mono text-sm">
                {t("settings.deadRow", { table: op.table_name, id: op.row_id })}
              </span>
              {op.last_error && <span className="block text-sm text-destructive">{op.last_error}</span>}
            </span>
            <Button
              type="button"
              size="pos"
              variant="outline"
              disabled={resolve.isPending}
              onClick={() => resolve.mutate({ seq: op.seq, action: "retry" })}
            >
              {t("settings.retry")}
            </Button>
            <Button
              type="button"
              size="pos"
              variant="destructive"
              disabled={resolve.isPending}
              onClick={() => resolve.mutate({ seq: op.seq, action: "discard" })}
            >
              {t("settings.discard")}
            </Button>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Settings → Sync (doc §1.3, §6): status, "Sync now", registration, refused changes and recent activity. */
export function SyncSettings() {
  const { t, i18n } = useTranslation("sync");
  const lang = i18n.resolvedLanguage ?? "en";
  const status = useSyncStatus();
  const info = useSyncInfo();
  const logs = useSyncLogs(10);
  const syncNow = useSyncNow();
  const [registerAgain, setRegisterAgain] = useState(false);
  const s = status.data;
  const registered = info.data?.registered ?? false;
  const showForm = !registered || s?.state === "unauthorized" || registerAgain;
  return (
    <SettingsCard sectionId="sync" id="sync-title" icon={<CloudCog />} title={t("settings.title")}>
      <p className="text-base text-muted-foreground">{t("settings.intro")}</p>
      {s && (
        <dl className="grid grid-cols-1 gap-1 text-base sm:grid-cols-2">
          <dt className="sr-only">{t("settings.state")}</dt>
          <dd className="font-semibold">{t(`pill.${s.state}`, { count: s.pending })}</dd>
          <dd>
            {s.last_success_at
              ? t("settings.lastSuccess", { when: when(s.last_success_at, lang) })
              : t("settings.never")}
          </dd>
          <dd>{t("settings.pending", { count: s.pending })}</dd>
          <dd>{t("settings.dead", { count: s.dead })}</dd>
          {s.last_error && (
            <dd className="text-destructive sm:col-span-2">
              {t("settings.lastError", { error: s.last_error })}
            </dd>
          )}
        </dl>
      )}
      {info.data?.server_url && (
        <p className="font-mono text-sm break-all text-muted-foreground">
          {t("settings.server", { url: info.data.server_url })}
          {info.data.device_code ? ` · ${t("settings.counter", { code: info.data.device_code })}` : ""}
        </p>
      )}
      {s?.chain_broken.map((c) => (
        <p key={c.device_code} role="alert" className="rounded-md bg-warning p-3 text-warning-foreground">
          {t("settings.chainBroken", { code: c.device_code, seq: c.seq ?? "?" })}
        </p>
      ))}
      {registered && (
        <Button
          type="button"
          size="pos"
          className="self-start"
          disabled={syncNow.isPending}
          onClick={() => syncNow.mutate()}
        >
          {t("settings.syncNow")}
        </Button>
      )}
      <Can perm="SYNC_MANAGE">
        {showForm ? (
          <RegisterForm again={registered} defaultUrl={info.data?.server_url ?? ""} />
        ) : (
          <Button
            type="button"
            size="pos"
            variant="outline"
            className="self-start"
            onClick={() => setRegisterAgain(true)}
          >
            {t("settings.reregisterTitle")}
          </Button>
        )}
        <DeadOps />
      </Can>
      <section className="flex flex-col gap-2" aria-labelledby="sync-log-title">
        <h3 id="sync-log-title" className="text-lg font-semibold">
          {t("settings.logTitle")}
        </h3>
        {logs.data?.length ? (
          <ul className="flex flex-col gap-1 text-sm">
            {logs.data.map((l) => (
              <li key={l.id} className="flex flex-wrap gap-2">
                <span className="font-mono">{when(l.started_at, lang)}</span>
                <span>
                  {t("settings.logRow", {
                    direction: t(`settings.${l.direction === "push" ? "push" : "pull"}`),
                    result: l.error_code ?? l.result,
                    sent: l.rows_sent,
                    received: l.rows_received,
                    rejected: l.rows_rejected,
                  })}
                </span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-sm text-muted-foreground">{t("settings.noLog")}</p>
        )}
      </section>
    </SettingsCard>
  );
}
