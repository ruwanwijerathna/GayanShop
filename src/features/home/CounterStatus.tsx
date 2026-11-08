import { CircleAlert, CircleCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useAppInfo, useIntegrity } from "@/db/hooks";

/** Counter health card: version, schema, data folder and the startup integrity probes (doc §13.2). */
export function CounterStatus() {
  const { t } = useTranslation("app");
  const info = useAppInfo();
  const integrity = useIntegrity();
  const report = integrity.data;
  const healthy =
    report &&
    report.quick_check === "ok" &&
    report.pending_orders === 0 &&
    report.unreversed_voids === 0 &&
    !report.stale_sync_flag;

  return (
    <section className="flex max-w-2xl flex-col gap-4 rounded-xl border border-border bg-card p-6">
      <h1 className="text-2xl font-semibold">{t("status.title")}</h1>
      <dl className="grid grid-cols-[12rem_1fr] gap-x-4 gap-y-2">
        <dt className="text-muted-foreground">{t("status.version")}</dt>
        <dd className="font-mono">{info.data?.app_version ?? "…"}</dd>
        <dt className="text-muted-foreground">{t("status.schema")}</dt>
        <dd className="font-mono tabular">{info.data?.schema_version ?? "…"}</dd>
        <dt className="text-muted-foreground">{t("status.dataDir")}</dt>
        <dd className="font-mono text-sm break-all">{info.data?.data_dir ?? "…"}</dd>
        <dt className="text-muted-foreground">{t("status.integrity")}</dt>
        <dd>
          {report &&
            (healthy ? (
              <span className="inline-flex items-center gap-2 rounded bg-success-surface px-2 text-success-foreground">
                <CircleCheck aria-hidden className="size-5" /> {t("status.healthy")}
              </span>
            ) : (
              <span className="inline-flex items-center gap-2 rounded bg-warning px-2 text-warning-foreground">
                <CircleAlert aria-hidden className="size-5" /> {t("status.problems")}
              </span>
            ))}
        </dd>
      </dl>
    </section>
  );
}
