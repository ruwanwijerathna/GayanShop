// Products → "Short barcodes" (decision #29): every listed product without a barcode gets an 8-digit in-store
// one in a single step, so its labels print with bars and scan. Rust picks the codes, skips anything that
// already has a barcode, and audits each product.
import { Barcode } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { Product } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useAssignInStoreBarcodes } from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";

/** Products the action applies to: standard products that have no barcode yet. */
export function withoutBarcode(products: readonly Product[]): Product[] {
  return products.filter((p) => p.kind === "standard" && (p.barcode ?? "").trim() === "");
}

export function ShortBarcodesDialog({
  products,
  onClose,
  onDone,
}: {
  /** The products to code (already filtered with [`withoutBarcode`]). */
  products: readonly Product[];
  onClose: () => void;
  /** The ids that got a barcode, so the screen can select them for printing. */
  onDone: (ids: string[]) => void;
}) {
  const { t, i18n } = useTranslation(["products", "common"]);
  const assign = useAssignInStoreBarcodes();
  const count = products.length;

  const confirm = () => {
    assign.mutate(
      products.map((p) => p.id),
      {
        onSuccess: (done) => {
          playCue("complete");
          toast.success(t("shortBarcodes.done", { count: done.assigned.length }));
          onDone(done.assigned.map((a) => a.product_id));
          onClose();
        },
        onError: () => playCue("error"),
      },
    );
  };

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Barcode aria-hidden className="size-5" />
            {t("shortBarcodes.title")}
          </DialogTitle>
          <DialogDescription>{t("shortBarcodes.description", { count })}</DialogDescription>
        </DialogHeader>
        {assign.error instanceof CommandError && (
          <p role="alert" className="text-base text-destructive">
            {translateError(i18n.t, assign.error.error)}
          </p>
        )}
        <DialogFooter className="gap-2">
          <Button type="button" size="pos" variant="outline" onClick={onClose}>
            {t("common:cancel")}
          </Button>
          <Button type="button" size="pos" disabled={assign.isPending || count === 0} onClick={confirm}>
            {t("shortBarcodes.confirm", { count })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
