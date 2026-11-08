import type { ColumnDef } from "@tanstack/react-table";
import { ShieldCheck } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { DataTable } from "@/components/data-table/DataTable";
import { Button } from "@/components/ui/button";
import type { AuditLogRow, ChainReport } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useAuditPages, useAuditVerify } from "@/db/hooks";
import { translateError } from "@/i18n";

/**
 * Column widths (px). Together they must fit the table at the 1024×768 minimum window: 911 px grid
 * minus the vertical scrollbar. Sized for the widest content: an en-LK timestamp and the longest si action label.
 */
export const AUDIT_COLUMN_WIDTHS = {
  when: 216,
  action: 232,
  staff: 150,
  approvedBy: 150,
  entity: 120,
} as const;
export const AUDIT_TABLE_BUDGET = 895;

function ChainResult({ report }: { report: ChainReport }) {
  const { t } = useTranslation("app");
  const device = report.device_id.slice(0, 8);
  if (report.ok) {
    return (
      <li className="text-success-foreground">{t("audit.chainOk", { device, rows: report.rows_checked })}</li>
    );
  }
  const brk = report.first_break;
  return (
    <li className="font-semibold text-destructive">
      {t("audit.chainBroken", {
        device,
        seq: brk?.device_seq ?? "?",
        reason: brk ? t(`audit.reasons.${brk.reason}`) : "",
      })}
    </li>
  );
}

/** Audit trail viewer (AUDIT_VIEW): virtualised keyset pages, newest first, plus chain verification. */
export function AuditLogScreen() {
  const { t, i18n } = useTranslation(["app", "common"]);
  const pages = useAuditPages();
  const verify = useAuditVerify();
  const rows = useMemo(() => pages.data?.pages.flatMap((p) => p.rows) ?? [], [pages.data]);

  const columns = useMemo<ColumnDef<AuditLogRow>[]>(
    () => [
      {
        id: "when",
        header: t("audit.when"),
        size: AUDIT_COLUMN_WIDTHS.when,
        accessorFn: (r) =>
          new Date(r.created_at).toLocaleString(i18n.resolvedLanguage === "si" ? "si-LK" : "en-LK"),
        enableSorting: false,
      },
      {
        id: "action",
        header: t("audit.action"),
        size: AUDIT_COLUMN_WIDTHS.action,
        accessorFn: (r) =>
          i18n.exists(`app:audit.actions.${r.action}`) ? t(`audit.actions.${r.action}`) : r.action,
        enableSorting: false,
      },
      {
        id: "staff",
        header: t("audit.staff"),
        size: AUDIT_COLUMN_WIDTHS.staff,
        accessorFn: (r) => r.staff_name ?? "—",
        enableSorting: false,
      },
      {
        id: "approvedBy",
        header: t("audit.approvedBy"),
        size: AUDIT_COLUMN_WIDTHS.approvedBy,
        accessorFn: (r) => r.approver_name ?? "",
        enableSorting: false,
      },
      {
        id: "entity",
        header: t("audit.entity"),
        size: AUDIT_COLUMN_WIDTHS.entity,
        accessorFn: (r) => (r.entity_type ? `${r.entity_type}` : ""),
        enableSorting: false,
      },
    ],
    [t, i18n],
  );

  const loadError = pages.error instanceof CommandError ? translateError(i18n.t, pages.error.error) : null;
  const verifyError =
    verify.error instanceof CommandError ? translateError(i18n.t, verify.error.error) : null;

  return (
    <section className="flex flex-col gap-4">
      <header className="flex items-center gap-4">
        <h1 className="flex-1 text-2xl font-semibold">{t("audit.title")}</h1>
        <Button
          type="button"
          size="pos"
          variant="outline"
          disabled={verify.isPending}
          onClick={() => verify.mutate()}
        >
          <ShieldCheck aria-hidden />
          {verify.isPending ? t("audit.verifying") : t("audit.verify")}
        </Button>
      </header>
      {verify.data && (
        <ul aria-live="polite" className="rounded-md border border-border p-3">
          {verify.data.map((r) => (
            <ChainResult key={r.device_id} report={r} />
          ))}
        </ul>
      )}
      {(loadError ?? verifyError) && (
        <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
          {loadError ?? verifyError}
        </p>
      )}
      <DataTable<AuditLogRow>
        ariaLabel={t("audit.title")}
        columns={columns}
        data={rows}
        getRowId={(r) => r.id}
        height={520}
        emptyLabel={pages.isLoading ? t("common:table.loading") : undefined}
        onEndReached={() => {
          if (pages.hasNextPage && !pages.isFetchingNextPage) void pages.fetchNextPage();
        }}
      />
    </section>
  );
}
