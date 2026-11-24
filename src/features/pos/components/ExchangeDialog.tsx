import { Loader2, Minus, Plus, Receipt, Search, X } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
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
 * F12: item-for-item exchange. The cashier finds the earlier sale by receipt number, picks the
 * units handed back and returns to the till, where the replacement goods are scanned as an ordinary cart.
 */
export function ExchangeDialog({ draft, initialReceipt = null, onConfirm, onClose }: ExchangeDialogProps) {
  const { t, i18n } = useTranslation("pos");
  const lookup = useExchangeLookup();
  const [receipt, setReceipt] = useState(initialReceipt ?? draft?.source.receipt_no ?? "");
  const [source, setSource] = useState<ExchangeSource | null>(draft?.source ?? null);
  const [picks, setPicks] = useState<ReturnPicks>(draft?.picks ?? {});
  const receiptRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Suggestions for bills containing what is typed
  const [suggesting, setSuggesting] = useState(false);
  const [active, setActive] = useState(-1);
  const debouncedReceipt = useDebounced(receipt, SUGGEST_DELAY_MS);
  const suggestions = useBillSuggestions(debouncedReceipt);
  const offered = suggesting && receipt.trim() !== "" ? (suggestions.data ?? []) : [];

  const search = (no: string) => {
    const trimmed = no.trim();
    if (trimmed === "" || lookup.isPending) return;
    lookup.mutate(trimmed, {
      onSuccess: (s) => {
        playCue("tick");
        setSource(s);
        setPicks({});
        setSuggesting(false);
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
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      if (offered.length === 0) return;
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setActive((prev) => {
        const next = prev + step;
        if (next >= offered.length) return 0;
        if (next < 0) return offered.length - 1;
        return next;
      });
      return;
    }

    if (e.key === "Enter") {
      e.preventDefault();
      if (active >= 0 && offered[active]) {
        pick(offered[active].receipt_no);
      } else if (receipt.trim() !== "") {
        setSuggesting(false);
        search(receipt.trim());
      }
      return;
    }

    if (e.key === "Escape") {
      if (suggesting && offered.length > 0) {
        e.preventDefault();
        setSuggesting(false);
      }
    }
  };

  // Scroll active suggestion into view when navigating via arrow keys
  useEffect(() => {
    if (active >= 0) {
      const el = document.getElementById(`exchange-suggestion-${active}`);
      el?.scrollIntoView({ block: "nearest" });
    }
  }, [active]);

  // Close suggestions on outside click
  useEffect(() => {
    const handleOutsideClick = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setSuggesting(false);
      }
    };
    document.addEventListener("mousedown", handleOutsideClick);
    return () => document.removeEventListener("mousedown", handleOutsideClick);
  }, []);

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
        className="w-full max-w-xl gap-4 p-5 sm:p-6"
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
        <DialogHeader className="pb-1 border-b">
          <DialogTitle className="text-xl font-bold flex items-center gap-2">
            <Receipt className="size-5 text-primary" />
            <span>{t("exchange.title")}</span>
          </DialogTitle>
        </DialogHeader>

        <form onSubmit={find} className="flex flex-col gap-2">
          <div ref={containerRef} className="relative w-full">
            <Label htmlFor="exchange-receipt" className="text-sm font-semibold mb-1.5 block">
              {t("exchange.receipt")}
            </Label>
            <div className="relative flex items-center">
              <Search
                aria-hidden
                className="pointer-events-none absolute left-3.5 size-4 text-muted-foreground"
              />
              <Input
                id="exchange-receipt"
                ref={receiptRef}
                className="h-12 pl-10 pr-10 font-mono text-base rounded-xl"
                autoComplete="off"
                placeholder={t("exchange.receiptPlaceholder", {
                  defaultValue: "Type or scan bill number (e.g. T01-261124004 or T01-4)",
                })}
                role="combobox"
                aria-expanded={offered.length > 0}
                aria-controls="exchange-suggestions"
                aria-autocomplete="list"
                aria-activedescendant={active >= 0 ? `exchange-suggestion-${active}` : undefined}
                value={receipt}
                onFocus={() => {
                  if (receipt.trim().length > 0) setSuggesting(true);
                }}
                onChange={(e) => {
                  setReceipt(e.target.value);
                  setSuggesting(true);
                  setActive(-1);
                }}
                onKeyDown={onKeyDown}
              />
              {lookup.isPending ? (
                <Loader2
                  aria-hidden
                  className="pointer-events-none absolute right-3.5 size-4 animate-spin text-muted-foreground"
                />
              ) : receipt.trim().length > 0 ? (
                <button
                  type="button"
                  tabIndex={-1}
                  className="absolute right-3.5 text-muted-foreground hover:text-foreground"
                  onClick={() => {
                    setReceipt("");
                    setSource(null);
                    setPicks({});
                    setSuggesting(false);
                    receiptRef.current?.focus();
                  }}
                  aria-label="Clear"
                >
                  <X className="size-4" />
                </button>
              ) : null}
            </div>

            {/* Floating Suggestions Dropdown */}
            {suggesting && offered.length > 0 && (
              <div
                id="exchange-suggestions-container"
                className="absolute left-0 right-0 top-full z-50 mt-1.5 max-h-72 overflow-y-auto overflow-x-hidden rounded-2xl border bg-popover p-1.5 shadow-2xl ring-1 ring-black/5"
              >
                <ul
                  id="exchange-suggestions"
                  role="listbox"
                  aria-label={t("exchange.suggestions")}
                  className="flex flex-col gap-1.5"
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
                        onMouseEnter={() => setActive(i)}
                        onClick={() => pick(b.receipt_no)}
                        className={cn(
                          "flex items-center justify-between gap-3 w-full rounded-xl px-3.5 py-2 text-left transition-all",
                          i === active
                            ? "bg-primary text-primary-foreground shadow-sm"
                            : "hover:bg-accent/70 text-foreground border border-transparent hover:border-border/40",
                        )}
                      >
                        <div className="flex items-center gap-3 min-w-0">
                          <div
                            className={cn(
                              "flex size-9 shrink-0 items-center justify-center rounded-lg transition-colors",
                              i === active
                                ? "bg-primary-foreground/15 text-primary-foreground"
                                : "bg-primary/10 text-primary dark:bg-primary/20",
                            )}
                          >
                            <Receipt className="size-4" />
                          </div>
                          <div className="flex flex-col min-w-0">
                            <span className="font-mono text-sm sm:text-base font-bold tracking-tight truncate">
                              {b.receipt_no}
                            </span>
                            <div
                              className={cn(
                                "flex items-center gap-1.5 text-xs tabular-nums",
                                i === active ? "text-primary-foreground/80" : "text-muted-foreground",
                              )}
                            >
                              <span>{b.business_date}</span>
                              <span className="opacity-40">•</span>
                              <span>
                                {t("exchange.itemCount", {
                                  count: b.item_count,
                                  defaultValue: `${b.item_count} ${b.item_count === 1 ? "item" : "items"}`,
                                })}
                              </span>
                            </div>
                          </div>
                        </div>

                        <div className="text-right shrink-0 pl-2">
                          <span
                            className={cn(
                              "font-mono text-base sm:text-lg font-bold tabular-nums tracking-tight block",
                              i === active ? "text-primary-foreground" : "text-foreground",
                            )}
                          >
                            {formatMoney(b.grand_total)}
                          </span>
                        </div>
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </div>
        </form>

        {error && (
          <p
            role="alert"
            className="text-base text-destructive bg-destructive/10 p-3 rounded-lg border border-destructive/20"
          >
            {error}
          </p>
        )}

        {source && (
          <div className="flex flex-col gap-3 pt-1">
            <div className="flex items-center justify-between border-b pb-2.5">
              <div className="flex items-center gap-2">
                <Receipt className="size-4 text-muted-foreground shrink-0" />
                <p className="text-sm sm:text-base font-semibold">
                  {t("exchange.sale", {
                    receipt: source.receipt_no,
                    date: source.business_date,
                    days: source.days_since,
                  })}
                </p>
              </div>
              {source.late && (
                <Badge variant="outline" className="border-amber-500 text-amber-700 bg-amber-50 font-medium">
                  {t("exchange.late", { days: source.window_days })}
                </Badge>
              )}
            </div>

            {!anyEligible && <p className="text-base text-muted-foreground">{t("exchange.nothing")}</p>}

            <ul
              className="flex max-h-72 flex-col gap-2.5 overflow-y-auto pr-1"
              aria-label={t("exchange.lines")}
            >
              {source.lines.map((l) => {
                const picked = picks[l.item_id] ?? 0;
                const unitPrice = Math.round(l.line_total / l.qty);
                return (
                  <li
                    key={l.item_id}
                    className={cn(
                      "flex flex-col gap-2.5 rounded-xl border p-3.5 transition-all shadow-xs",
                      picked > 0
                        ? "border-primary/50 bg-primary/[0.04] ring-1 ring-primary/20"
                        : "border-border/70 bg-card hover:border-border",
                      !l.eligible && "opacity-60 bg-muted/20",
                    )}
                  >
                    {/* Top row: Item Name and Line Price */}
                    <div className="flex items-start justify-between gap-3 min-w-0">
                      <div className="min-w-0 flex-1">
                        <h4 className="text-base font-semibold leading-snug tracking-tight text-foreground truncate">
                          {l.name}
                        </h4>
                        <p className="text-xs text-muted-foreground mt-0.5">
                          {l.eligible
                            ? t("exchange.returnable", { left: l.returnable_qty, sold: l.qty })
                            : l.returned_qty >= l.qty
                              ? t("exchange.alreadyReturned")
                              : t("exchange.notReturnable")}
                        </p>
                      </div>
                      <div className="text-right shrink-0">
                        <span className="font-mono text-base font-bold tabular-nums text-foreground block">
                          {formatMoney(l.line_total)}
                        </span>
                        {l.qty > 1 && (
                          <span className="text-xs text-muted-foreground font-mono tabular-nums">
                            {l.qty} × {formatMoney(unitPrice)}
                          </span>
                        )}
                      </div>
                    </div>

                    {/* Bottom row: Return Stepper and Credit Amount */}
                    <div className="flex items-center justify-between border-t border-dashed border-border/60 pt-2.5 mt-0.5">
                      <span className="text-xs font-medium">
                        {picked > 0 ? (
                          <span className="text-primary font-semibold">
                            {t("exchange.returned", { units: picked })}
                          </span>
                        ) : (
                          <span className="text-muted-foreground font-mono">
                            {formatMoney(unitPrice)} / unit
                          </span>
                        )}
                      </span>

                      <div className="flex items-center gap-3">
                        {picked > 0 && (
                          <span className="font-mono font-bold text-sm text-primary tabular-nums">
                            + {formatMoney(lineCredit(l, picked))}
                          </span>
                        )}
                        <div className="flex items-center gap-1.5 rounded-lg border bg-background p-0.5 shadow-xs">
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="size-8 rounded-md hover:bg-muted text-muted-foreground hover:text-foreground"
                            aria-label={t("exchange.fewer", { name: l.name })}
                            disabled={!l.eligible || picked === 0}
                            onClick={() => change(l.item_id, -1, l.returnable_qty)}
                          >
                            <Minus aria-hidden className="size-3.5" />
                          </Button>
                          <span
                            className="w-7 text-center font-mono text-sm font-bold tabular-nums"
                            aria-live="polite"
                          >
                            {picked}
                          </span>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="size-8 rounded-md hover:bg-muted text-muted-foreground hover:text-foreground"
                            aria-label={t("exchange.more", { name: l.name })}
                            disabled={!l.eligible || picked >= l.returnable_qty}
                            onClick={() => change(l.item_id, 1, l.returnable_qty)}
                          >
                            <Plus aria-hidden className="size-3.5" />
                          </Button>
                        </div>
                      </div>
                    </div>
                  </li>
                );
              })}
            </ul>

            <div className="flex items-center justify-between rounded-xl border bg-primary/5 border-primary/20 px-4 py-3 shadow-xs">
              <div className="flex flex-col gap-0.5">
                <span className="text-sm font-semibold text-foreground">
                  {t("exchange.credit", { units })}
                </span>
                <span className="text-xs text-muted-foreground">
                  {t("exchange.rule")}
                </span>
              </div>
              <span
                className="text-2xl font-bold font-mono text-primary tabular-nums shrink-0 ml-3"
                data-testid="exchange-credit"
              >
                {formatMoney(credit)}
              </span>
            </div>
          </div>
        )}

        <DialogFooter className="gap-2 sm:justify-end pt-2 border-t">
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
