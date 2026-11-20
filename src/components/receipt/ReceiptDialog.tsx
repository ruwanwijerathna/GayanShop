// A sealed order's receipt on screen (back-office drill-down from order or customer history). The phone is
// always masked, as on paper (doc §3.8).
import { Printer } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import type { ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { translateError } from "@/i18n";
import { SinhalaReceiptTemplate } from "./SinhalaReceiptTemplate";
import { usePrintReceiptStore } from "./PrintableReceiptPortal";

export function ReceiptDialog({
  receipt,
  error,
  onClose,
}: {
  receipt: ReceiptData | undefined;
  error: unknown;
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation("common");
  const r = receipt;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[92vh] overflow-y-auto sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("receipt.title", { no: r?.receipt_no ?? "" })}</DialogTitle>
        </DialogHeader>
        {error instanceof CommandError && (
          <p role="alert" className="text-base text-destructive">
            {translateError(i18n.t, error.error)}
          </p>
        )}
        {r && (
          <div className="flex flex-col items-center">
            <div className="w-full rounded-md border border-border bg-neutral-100/60 p-2 shadow-sm">
              <SinhalaReceiptTemplate receipt={r} />
            </div>
          </div>
        )}
        <DialogFooter className="flex flex-row items-center justify-between gap-2 sm:justify-between">
          {r && (
            <Button
              type="button"
              size="pos"
              className="flex-1"
              onClick={() => usePrintReceiptStore.getState().print(r)}
            >
              <Printer className="mr-2 size-4" />
              {t("receipt.print", { defaultValue: "මුද්‍රණය කරන්න (Print)" })}
            </Button>
          )}
          <Button type="button" size="pos" variant="outline" onClick={onClose}>
            {t("actions.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
