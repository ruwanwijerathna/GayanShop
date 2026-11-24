import { Minus, Plus } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
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
import { useBillSuggestions, useExchangeLookup } from "@/db/hooks";
import { useDebounced } from "@/hooks/useDebounced";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";
import { formatMoney } from "@/lib/money";
import { cn } from "@/lib/utils";
import { lineCredit, pickedUnits, totalCredit, type ReturnPicks } from "../lib/exchange";
import type { ExchangeDraft } from "../store/cart.store";

const SUGGEST_DELAY_MS = 150;

export interface ExchangeDialogProps {
  /** The exchange already started on this cart, to change its picks. */
  draft: ExchangeDraft | null;
  /** A bill number scanned at the till: looked up straight away. */
  initialReceipt?: string | null;
  onConfirm: (draft: ExchangeDraft) => void;
  onClose: () => void;
}

/**
 * F12 (decision #26): item-for-item exchange. The cashier finds the earlier sale by receipt number, picks the
 * units handed back and returns to the till, where the replacement goods are scanned as an ordinary cart. The
 * customer pays only the difference; there is never a cash refund or a credit note. After the 7-day window the
 * sale needs a supervisor or owner PIN, asked for at payment.
 */
export function ExchangeDialog({ draft, initialReceipt = null, onConfirm, onClose }: ExchangeDialogProps) {
  const { t, i18n } = useTranslation("pos");
  const lookup = useExchangeLookup();
  const [receipt, setReceipt] = useState(initialReceipt ?? draft?.source.receipt_no ?? "");
  const [source, setSource] = useState<ExchangeSource | null>(draft?.source ?? null);
  const [picks, setPicks] = useState<ReturnPicks>(draft?.picks ?? {});
  const receiptRef = useRef<HTMLInputElement>(null);
  // Bills containing what is typed are offered under the field; picking one looks it up at once.
  const [suggesting, setSuggesting] = useState(false);
  const [active, setActive] = useState(-1);
  const suggestions = useBillSuggestions(useDebounced(receipt, SUGGEST_DELAY_MS));
  const offered = suggesting && receipt.trim() !== "" ? (suggestions.data ?? []) : [];

  const search = (no: string) => {
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

  const find = (e: FormEvent) => {
    e.preventDefault();
    setSuggesting(false);
    search(receipt.trim());
  };

  const pick = (no: string) => {
    setReceipt(no);
    setSuggesting(false);
    setActive(-1);
    search(no);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (offered.length === 0) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setActive((a) => {
        const next = a + step;
        if (next >= offered.length) return -1;
        return next < -1 ? offered.length - 1 : next;
      });
    } else if (e.key === "Enter" && active >= 0 && offered[active]) {
      e.preventDefault();
      pick(offered[active].receipt_no);
    }
  };

  const scanned = useRef(initialReceipt);
  useEffect(() => {
    const no = scanned.current;
    scanned.current = null;
    if (no) search(no);
    // Runs once: the receipt scanned at the till.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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
        onEscapeKeyDown={(e) => {
          // First Esc closes the suggestions, the next one the dialog.
          if (offered.length > 0) {
            e.preventDefault();
            setSuggesting(false);
          }
        }}
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
              placeholder="T01-261112001"
              role="combobox"
              aria-expanded={offered.length > 0}
              aria-controls="exchange-suggestions"
              aria-autocomplete="list"
              aria-activedescendant={active >= 0 ? `exchange-suggestion-${active}` : undefined}
              value={receipt}
              onChange={(e) => {
                setReceipt(e.target.value);
                setSuggesting(true);
                setActive(-1);
              }}
              onKeyDown={onKeyDown}
            />
            {offered.length > 0 ? (
              <ul
                id="exchange-suggestions"
                role="listbox"
                aria-label={t("exchange.suggestions")}
                className="flex flex-col gap-1 rounded-md border bg-card p-1"
              >
                {offered.map((b, i) => (
                  <li
                    key={b.receipt_no}
                    id={`exchange-suggestion-${i}`}
                    role="option"
                    aria-selected={i === active}
                  >
                    <button
                      type="button"
                      tabIndex={-1}
                      onClick={() => pick(b.receipt_no)}
                      className={cn(
                        "flex min-h-12 w-full items-center justify-between gap-3 rounded-sm px-3 text-left hover:bg-accent",
                        i === active && "bg-accent",
                      )}
                    >
                      <span className="font-mono text-base">{b.receipt_no}</span>
                      <span className="text-sm text-muted-foreground">
                        {t("exchange.suggestion", {
                          date: b.business_date,
                          count: b.item_count,
                          total: formatMoney(b.grand_total),
                        })}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-sm text-muted-foreground">{t("exchange.receiptHint")}</p>
            )}
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
