// In-app Receipt Preview Dialog: displays the styled receipt within a centered dialog
// with vertical scrolling for long bills, and actions to print or close.
import { Printer } from "lucide-react";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import type { ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { usePrinterProfile } from "@/db/hooks";
import { translateError } from "@/i18n";
import { SinhalaReceiptTemplate } from "./SinhalaReceiptTemplate";
import { usePrintReceiptStore } from "./PrintableReceiptPortal";

export interface ReceiptDialogProps {
  receipt: ReceiptData | undefined | null;
  error?: unknown;
  onClose: () => void;
  isReprint?: boolean;
}

export function ReceiptDialog({ receipt, error, onClose, isReprint = true }: ReceiptDialogProps) {
  const { t, i18n } = useTranslation("common");
  const profile = usePrinterProfile();
  const r = receipt;

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Enter" && r) {
        e.preventDefault();
        usePrintReceiptStore.getState().print(r, { reprint: isReprint });
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [r, isReprint, onClose]);

  return (
    <Dialog open={Boolean(r || error)} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="w-full max-w-md max-h-[calc(100vh-3.5rem)] flex flex-col p-4 sm:p-5 gap-3 rounded-xl overflow-hidden shadow-2xl">
        <DialogHeader className="shrink-0 pb-2 border-b pr-8">
          <DialogTitle className="flex items-center gap-2 text-lg">
            <span>{t("receipt.previewTitle", { defaultValue: "Receipt Preview" })}</span>
            {r?.receipt_no && (
              <span className="font-mono text-xs font-normal text-muted-foreground bg-muted px-2 py-0.5 rounded">
                #{r.receipt_no}
              </span>
            )}
          </DialogTitle>
        </DialogHeader>
        {error instanceof CommandError && (
          <p role="alert" className="text-base text-destructive shrink-0">
            {translateError(i18n.t, error.error)}
          </p>
        )}
        {r && (
          <div className="flex-1 min-h-0 overflow-y-auto px-2 py-2 bg-neutral-100/70 rounded-md border flex justify-center items-start">
            <div className="w-full max-w-[72mm] bg-white shadow-sm rounded border border-border h-fit">
              <SinhalaReceiptTemplate receipt={r} qr={profile.data?.qr} paperWidth={profile.data?.width} />
            </div>
          </div>
        )}
        <DialogFooter className="shrink-0 flex flex-row items-center justify-between gap-3 pt-2 border-t sm:justify-between">
          {r && (
            <Button
              type="button"
              size="pos"
              className="flex-1 text-base font-semibold"
              onClick={() => {
                usePrintReceiptStore.getState().print(r, { reprint: isReprint });
                onClose();
              }}
            >
              <Printer className="mr-2 size-5" />
              {t("receipt.print", { defaultValue: "Print" })}
            </Button>
          )}
          <Button type="button" size="pos" variant="outline" onClick={onClose}>
            {t("actions.close", { defaultValue: "Close" })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export { ReceiptDialog as ReceiptPreviewDialog };
