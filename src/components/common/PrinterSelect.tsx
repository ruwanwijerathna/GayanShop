import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { SelectField } from "@/components/common/SelectField";
import { Button } from "@/components/ui/button";
import { useSystemPrinters } from "@/db/hooks";
import { cn } from "@/lib/utils";

/**
 * A dropdown of the printers installed in Windows with a Refresh button for one that was just plugged in.
 * A saved printer that is no longer installed stays in the list, marked, so it isn't silently lost.
 */
export function PrinterSelect({
  id,
  value,
  onChange,
}: {
  id: string;
  value: string;
  onChange: (name: string) => void;
}) {
  const { t } = useTranslation("common");
  const printers = useSystemPrinters();
  const installed = printers.data ?? [];
  const missing = value !== "" && printers.data !== undefined && !installed.includes(value);
  const options = [
    ...(value === "" ? [{ value: "", label: t("printerSelect.choose") }] : []),
    ...(missing ? [{ value, label: t("printerSelect.missing", { name: value }) }] : []),
    ...installed.map((name) => ({ value: name, label: name })),
  ];

  return (
    <div className="flex flex-col gap-2">
      <div className="flex gap-2">
        <SelectField
          id={id}
          value={value}
          onValueChange={onChange}
          options={options}
          disabled={options.length === 0}
          className="min-w-0 flex-1"
        />
        <Button
          type="button"
          variant="outline"
          size="icon-pos"
          aria-label={t("printerSelect.refresh")}
          disabled={printers.isFetching}
          onClick={() => void printers.refetch()}
        >
          <RefreshCw aria-hidden className={cn(printers.isFetching && "motion-safe:animate-spin")} />
        </Button>
      </div>
      {printers.isError && (
        <p role="alert" className="text-sm text-destructive">
          {t("printerSelect.failed")}
        </p>
      )}
      {printers.isSuccess && installed.length === 0 && (
        <p className="text-sm text-muted-foreground">{t("printerSelect.none")}</p>
      )}
    </div>
  );
}
