import { Minus, Plus } from "lucide-react";
import { useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { ExchangeSource } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useExchangeLookup } from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";
import { formatMoney } from "@/lib/money";
import { cn } from "@/lib/utils";
import { lineCredit, pickedUnits, totalCredit, type ReturnPicks } from "../lib/exchange";
import type { ExchangeDraft } from "../store/cart.store";

export interface ExchangeDialogProps {
  /** The exchange already started on this cart, to change its picks. */
  draft: ExchangeDraft | null;
  onConfirm: (draft: ExchangeDraft) => void;
  onClose: () => void;
}

/**
 * F12 (decision #26): item-for-item exchange. The cashier finds the earlier sale by receipt number, picks the
 * units handed back and returns to the till, where the replacement goods are scanned as an ordinary cart. The
 * customer pays only the difference; there is never a cash refund or a credit note. After the 7-day window the
 * sale needs a supervisor or owner PIN, asked for at payment.
 */
export function ExchangeDialog({ draft, onConfirm, onClose }: ExchangeDialogProps) {
  const { t, i18n } = useTranslation("pos");
  const lookup = useExchangeLookup();
  const [receipt, setReceipt] = useState(draft?.source.receipt_no ?? "");
  const [source, setSource] = useState<ExchangeSource | null>(draft?.source ?? null);
  const [picks, setPicks] = useState<ReturnPicks>(draft?.picks ?? {});
  const receiptRef = useRef<HTMLInputElement>(null);

  const find = (e: FormEvent) => {
    e.preventDefault();
    const no = receipt.trim();
    if (no === "" || lookup.isPending) return;
    lookup.mutate(no, {
      onSuccess: (s) => {
        playCue("tick");
        setSource(s);
        setPicks({});
      },
      onError: () => playCue("error"),
    });
  };

  const change = (itemId: string, delta: number, max: number) =>
    setPicks((p) => {
      const next = Math.min(max, Math.max(0, (p[itemId] ?? 0) + delta));
      return { ...p, [itemId]: next };
    });

  const credit = source ? totalCredit(source, picks) : 0;
  const units = pickedUnits(picks);
  const error = lookup.error instanceof CommandError ? translateError(i18n.t, lookup.error.error) : null;
  const anyEligible = source?.lines.some((l) => l.eligible) ?? false;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        className="max-w-3xl"
        onOpenAutoFocus={(e) => {
          e.preventDefault();
          receiptRef.current?.focus();
        }}
      >
        <DialogHeader>
          <DialogTitle className="text-xl">{t("exchange.title")}</DialogTitle>
          <DialogDescription>{t("exchange.help")}</DialogDescription>
        </DialogHeader>

        <form onSubmit={find} className="flex flex-wrap items-end gap-3">
          <div className="flex min-w-64 flex-1 flex-col gap-2">
            <Label htmlFor="exchange-receipt">{t("exchange.receipt")}</Label>
            <Input
              id="exchange-receipt"
              ref={receiptRef}
              className="h-12 font-mono text-base"
              autoComplete="off"
              placeholder="T01-20261112-0001"
              value={receipt}
              onChange={(e) => setReceipt(e.target.value)}
            />
          </div>
          <Button type="submit" size="pos" disabled={receipt.trim() === "" || lookup.isPending}>
            {t("exchange.find")}
          </Button>
        </form>
        {error && (
          <p role="alert" className="text-base text-destructive">
            {error}
          </p>
        )}

        {source && (
          <div className="flex flex-col gap-3">
            <p className="text-base">
              {t("exchange.sale", {
                receipt: source.receipt_no,
                date: source.business_date,
                days: source.days_since,
              })}
            </p>
            {source.late && (
              <p role="status" className="rounded-md border bg-warning/15 px-3 py-2 text-base">
                {t("exchange.late", { days: source.window_days })}
              </p>
            )}
            {!anyEligible && <p className="text-base text-muted-foreground">{t("exchange.nothing")}</p>}
            <ul className="flex max-h-72 flex-col gap-2 overflow-y-auto" aria-label={t("exchange.lines")}>
              {source.lines.map((l) => {
                const picked = picks[l.item_id] ?? 0;
                return (
                  <li
                    key={l.item_id}
                    className={cn(
                      "flex min-h-14 items-center gap-3 rounded-md border px-3 py-1 text-base",
                      !l.eligible && "opacity-60",
                    )}
                  >
                    <span className="min-w-0 flex-1">
                      <span className="block truncate">{l.name}</span>
                      <span className="block text-sm text-muted-foreground">
                        {l.eligible
                          ? t("exchange.returnable", { left: l.returnable_qty, sold: l.qty })
                          : l.returned_qty >= l.qty
                            ? t("exchange.alreadyReturned")
                            : t("exchange.notReturnable")}
                      </span>
                    </span>
                    <span className="tabular-nums">{formatMoney(l.line_total)}</span>
                    <span className="flex items-center gap-2">
                      <Button
                        type="button"
                        variant="outline"
                        size="pos"
                        className="size-12 p-0"
                        aria-label={t("exchange.fewer", { name: l.name })}
                        disabled={!l.eligible || picked === 0}
                        onClick={() => change(l.item_id, -1, l.returnable_qty)}
                      >
                        <Minus aria-hidden />
                      </Button>
                      <span className="w-8 text-center text-lg tabular-nums" aria-live="polite">
                        {picked}
                      </span>
                      <Button
                        type="button"
                        variant="outline"
                        size="pos"
                        className="size-12 p-0"
                        aria-label={t("exchange.more", { name: l.name })}
                        disabled={!l.eligible || picked >= l.returnable_qty}
                        onClick={() => change(l.item_id, 1, l.returnable_qty)}
                      >
                        <Plus aria-hidden />
                      </Button>
                    </span>
                    <span className="w-28 text-right tabular-nums text-muted-foreground">
                      {picked > 0 ? formatMoney(lineCredit(l, picked)) : ""}
                    </span>
                  </li>
                );
              })}
            </ul>
            <p className="flex items-baseline justify-between text-lg font-semibold">
              <span>{t("exchange.credit", { units })}</span>
              <span className="tabular-nums" data-testid="exchange-credit">
                {formatMoney(credit)}
              </span>
            </p>
            <p className="text-sm text-muted-foreground">{t("exchange.rule")}</p>
          </div>
        )}

        <DialogFooter className="gap-2 sm:justify-end">
          <Button type="button" variant="outline" size="pos" onClick={onClose}>
            {i18n.t("common:actions.cancel", { defaultValue: "Cancel" })}
          </Button>
          <Button
            type="button"
            size="pos"
            disabled={!source || units === 0}
            onClick={() => source && onConfirm({ source, picks })}
          >
            {t("exchange.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
