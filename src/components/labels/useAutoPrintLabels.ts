// "Print labels when stock arrives" (Settings → Label printer). Each receiving flow hands over what arrived;
// when the switch is on this queues one sticker per unit on the roll chosen in Settings. The stock is already
// in by then, so a print failure only warns and never undoes the receipt.
import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { CommandError } from "@/db/client";
import { useLabelPrinterProfile, usePrintLabels } from "@/db/hooks";
import { translateError } from "@/i18n";
import { lastTemplate } from "./templatePreference";

export interface ArrivedStock {
  product_id: string;
  qty: number;
}

/** Most copies of one product in an automatic job (the print dialog's limit). */
const MAX_COPIES = 999;

export function useAutoPrintLabels(): (arrived: ArrivedStock[]) => void {
  const { t, i18n } = useTranslation("labels");
  const profile = useLabelPrinterProfile();
  const print = usePrintLabels();
  const enabled = profile.data?.auto_print_on_receive === true;
  const { mutate } = print;

  return useCallback(
    (arrived: ArrivedStock[]) => {
      if (!enabled) return;
      const totals = new Map<string, number>();
      for (const a of arrived) {
        if (a.qty > 0) totals.set(a.product_id, (totals.get(a.product_id) ?? 0) + a.qty);
      }
      if (totals.size === 0) return;
      const items = [...totals].map(([product_id, qty]) => ({
        product_id,
        copies: Math.min(qty, MAX_COPIES),
      }));
      const count = items.reduce((n, i) => n + i.copies, 0);
      mutate(
        { job: { template_id: lastTemplate(), items, fill_last_row: false }, approvalId: null },
        {
          onSuccess: () => toast.success(t("auto.queued", { count })),
          onError: (e) =>
            toast.error(
              e instanceof CommandError
                ? `${t("auto.failed")} ${translateError(i18n.t, e.error)}`
                : t("auto.failed"),
            ),
        },
      );
    },
    [enabled, mutate, t, i18n],
  );
}
