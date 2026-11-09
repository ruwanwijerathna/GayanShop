// "Export CSV" (doc §5, DATA_EXPORT, audited): pick a folder, write the file there, say where it went.
// Rust decides what the viewer may export (each kind also needs its screen's permission; costs need
// VIEW_COST), so a refusal comes back as a translated error.
import { FileDown } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Can } from "@/components/auth/Can";
import { Button } from "@/components/ui/button";
import type { ExportKind, Period } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { pickExportFolder, useExportCsv } from "@/db/hooks";
import { translateError } from "@/i18n";

export function ExportButton({ kind, period }: { kind: ExportKind; period: Period }) {
  const { t, i18n } = useTranslation("common");
  const run = useExportCsv();
  const [choosing, setChoosing] = useState(false);
  const start = async () => {
    setChoosing(true);
    try {
      const dir = await pickExportFolder();
      if (dir)
        run.mutate(
          { kind, period, dir },
          {
            onSuccess: (out) => toast.success(t("export.done", { count: out.rows, path: out.path })),
            onError: (e) =>
              toast.error(e instanceof CommandError ? translateError(i18n.t, e.error) : t("export.failed")),
          },
        );
    } catch (e) {
      toast.error(e instanceof CommandError ? translateError(i18n.t, e.error) : t("export.failed"));
    } finally {
      setChoosing(false);
    }
  };
  return (
    <Can perm="DATA_EXPORT">
      <Button
        type="button"
        size="pos"
        variant="outline"
        disabled={choosing || run.isPending}
        onClick={() => void start()}
      >
        <FileDown aria-hidden />
        {t(`export.kinds.${kind}`)}
      </Button>
    </Can>
  );
}
