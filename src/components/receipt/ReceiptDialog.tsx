// A sealed order's receipt on screen (back-office drill-down from order or customer history). The phone is
// always masked, as on paper (doc §3.8).
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { translateError } from "@/i18n";
import { formatMoney } from "@/lib/money";

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
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("receipt.title", { no: r?.receipt_no ?? "" })}</DialogTitle>
          <DialogDescription>
            {r
              ? `${r.business_date} · ${r.cashier}${r.customer_masked ? ` · ${r.customer_masked}` : ""}`
              : ""}
          </DialogDescription>
        </DialogHeader>
        {error instanceof CommandError && (
          <p role="alert" className="text-base text-destructive">
            {translateError(i18n.t, error.error)}
          </p>
        )}
        {r && (
          <div className="flex flex-col gap-3">
            {r.status === "voided" && (
              <Badge variant="destructive" className="self-start">
                {t("receipt.voided")}
              </Badge>
            )}
            <table className="w-full text-left text-sm">
              <thead className="border-b">
                <tr>
                  <th className="py-2">{t("receipt.item")}</th>
                  <th className="text-right">{t("receipt.qty")}</th>
                  <th className="text-right">{t("receipt.total")}</th>
                </tr>
              </thead>
              <tbody>
                {r.lines.map((l, i) => (
                  <tr key={i} className="h-10 border-b">
                    <td>{l.name}</td>
                    <td className="text-right font-mono">{l.qty}</td>
                    <td className="text-right font-mono">{formatMoney(l.line_total)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <dl className="grid grid-cols-2 gap-1 text-base">
              <dt>{t("receipt.discount")}</dt>
              <dd className="text-right font-mono">{formatMoney(r.discount_total)}</dd>
              <dt className="font-semibold">{t("receipt.grandTotal")}</dt>
              <dd className="text-right font-mono font-semibold">{formatMoney(r.grand_total)}</dd>
              {r.payments.map((p, i) => (
                <div key={i} className="contents">
                  <dt>{t(`receipt.methods.${p.method}`, { defaultValue: p.method })}</dt>
                  <dd className="text-right font-mono">{formatMoney(p.amount)}</dd>
                </div>
              ))}
              {r.change_due > 0 && (
                <>
                  <dt>{t("receipt.change")}</dt>
                  <dd className="text-right font-mono">{formatMoney(r.change_due)}</dd>
                </>
              )}
            </dl>
          </div>
        )}
        <DialogFooter>
          <Button type="button" size="pos" variant="outline" onClick={onClose}>
            {t("actions.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
