// "Export CSV" (doc §5, DATA_EXPORT, audited): pick a folder, write the file there, say where it went.
// Rust decides what the viewer may export (each kind also needs its screen's permission; costs need
// VIEW_COST), so a refusal comes back as a translated error.
import { FileDown } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Can } from "@/components/auth/Can";
import { Button } from "@/components/ui/button";
import { DropdownMenuItem } from "@/components/ui/dropdown-menu";
import type { ExportKind, Period } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { pickExportFolder, useExportCsv } from "@/db/hooks";
import { translateError } from "@/i18n";

function useExportAction(kind: ExportKind, period: Period) {
  const { t, i18n } = useTranslation("common");
  const run = useExportCsv();
  const [choosing, setChoosing] = useState(false);
  const start = async () => {
    setChoosing(true);
    try {
      const dir = await pickExportFolder();
      if (dir) {
        const out = await run.mutateAsync({ kind, period, dir });
        toast.success(t("export.done", { count: out.rows, path: out.path }));
      }
    } catch (e) {
      toast.error(e instanceof CommandError ? translateError(i18n.t, e.error) : t("export.failed"));
    } finally {
      setChoosing(false);
    }
  };
  return { start, busy: choosing || run.isPending, label: t(`export.kinds.${kind}`) };
}

export function ExportButton({ kind, period }: { kind: ExportKind; period: Period }) {
  const { start, busy, label } = useExportAction(kind, period);
  return (
    <Can perm="DATA_EXPORT">
      <Button type="button" size="pos" variant="outline" disabled={busy} onClick={() => void start()}>
        <FileDown aria-hidden />
        {label}
      </Button>
    </Can>
  );
}

export function ExportMenuItem({ kind, period }: { kind: ExportKind; period: Period }) {
  const { start, busy, label } = useExportAction(kind, period);
  return (
    <DropdownMenuItem className="min-h-11" disabled={busy} onSelect={() => void start()}>
      <FileDown aria-hidden />
      {label}
    </DropdownMenuItem>
  );
}
