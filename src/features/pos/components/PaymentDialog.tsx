import { Banknote, Plus, Trash2 } from "lucide-react";
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
import type { PaymentLine_Deserialize as PaymentLine, PaymentMethod } from "@/db/bindings";
import { useTouchMode } from "@/db/hooks";
import { formatMoney, parseMajor, toMajorInput, type Money } from "@/lib/money";
import { NumericKeypad } from "./NumericKeypad";
import { WithTooltip } from "@/components/common/WithTooltip";

const METHODS: PaymentMethod[] = ["cash", "card", "bank_transfer", "qr", "other"];

export interface PaymentRow {
  method: PaymentMethod;
  /** Cash: what the customer handed over (empty = exact). Others: the amount. */
  value: string;
  reference: string;
}

export type PaymentPlan =
  | { ok: true; payments: PaymentLine[]; change: Money; paid: Money }
  | { ok: false; reason: "notEnough" | "tooMuch" | "invalid"; paid: Money };

/**
 * Turn the rows into `checkout_order` payments (doc §3.7): every amount ≥ 0, their sum exactly the amount
 * due, change only from cash (cash `amount` = what is applied, `tendered` = what was handed over).
 */
export function planPayments(due: Money, rows: PaymentRow[]): PaymentPlan {
  const others = rows.filter((r) => r.method !== "cash");
  const cash = rows.find((r) => r.method === "cash");
  const payments: PaymentLine[] = [];
  let nonCash = 0;
  for (const r of others) {
    const amount = parseMajor(r.value);
    if (amount === null) return { ok: false, reason: "invalid", paid: nonCash };
    nonCash += amount;
    if (amount > 0)
      payments.push({ method: r.method, amount, tendered: null, reference: r.reference.trim() || null });
  }
  if (nonCash > due) return { ok: false, reason: "tooMuch", paid: nonCash };
  const remaining = due - nonCash;
  if (!cash) {
    return remaining === 0 && payments.length > 0
      ? { ok: true, payments, change: 0, paid: nonCash }
      : { ok: false, reason: "notEnough", paid: nonCash };
  }
  const tendered = cash.value.trim() === "" ? remaining : parseMajor(cash.value);
  if (tendered === null) return { ok: false, reason: "invalid", paid: nonCash };
  if (tendered < remaining) return { ok: false, reason: "notEnough", paid: nonCash + tendered };
  payments.push({ method: "cash", amount: remaining, tendered, reference: null });
  return { ok: true, payments, change: tendered - remaining, paid: nonCash + tendered };
}

/** The staff number typed at the till: whole numbers from 1 to 9999; anything else is not a number yet. */
export function parseStaffNo(text: string): number | null {
  const s = text.trim();
  return /^[1-9]\d{0,3}$/.test(s) ? Number.parseInt(s, 10) : null;
}

export interface PaymentDialogProps {
  open: boolean;
  due: Money;
  pending: boolean;
  error: string | null;
  onPay: (payments: PaymentLine[], soldByNo: number) => void;
  onCancel: () => void;
}

export function PaymentDialog({ open, due, pending, error, onPay, onCancel }: PaymentDialogProps) {
  const { t } = useTranslation("pos");
  const touchMode = useTouchMode();
  const [rows, setRows] = useState<PaymentRow[]>([{ method: "cash", value: "", reference: "" }]);
  const [soldBy, setSoldBy] = useState("");
  const soldByRef = useRef<HTMLInputElement>(null);
  const soldByNo = parseStaffNo(soldBy);
  const plan = planPayments(due, rows);
  const remaining = Math.max(0, due - plan.paid);

  const update = (i: number, patch: Partial<PaymentRow>) =>
    setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const add = (method: PaymentMethod) =>
    setRows((rs) => {
      const used = rs.reduce((a, r) => a + (parseMajor(r.value) ?? 0), 0);
      const left = Math.max(0, due - used);
      const value = method === "cash" ? "" : toMajorInput(left);
      return [...rs, { method, value, reference: "" }];
    });

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (plan.ok && soldByNo !== null && !pending) onPay(plan.payments, soldByNo);
  };

  const hasCash = rows.some((r) => r.method === "cash");

  return (
    <Dialog open={open} onOpenChange={(o) => !o && !pending && onCancel()}>
      <DialogContent
        className="max-w-3xl"
        onOpenAutoFocus={(e) => {
          // Who made the sale is the one required answer, so it takes the first keystrokes.
          e.preventDefault();
          soldByRef.current?.focus();
        }}
      >
        <DialogHeader>
          <DialogTitle className="text-xl">{t("pay.title")}</DialogTitle>
          <DialogDescription className="sr-only">{t("pay.due")}</DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className={touchMode ? "grid gap-6 md:grid-cols-[1fr_auto]" : "grid gap-6"}>
          <div className="flex flex-col gap-4">
            <dl className="grid grid-cols-2 gap-2">
              <dt className="text-base text-muted-foreground">{t("pay.due")}</dt>
              <dd className="text-right text-[32px] leading-tight font-bold tabular-nums">
                {formatMoney(due)}
              </dd>
              {plan.ok && plan.change > 0 ? (
                <>
                  <dt className="text-base text-muted-foreground">{t("pay.change")}</dt>
                  <dd className="text-right text-[32px] leading-tight font-bold text-success-foreground tabular-nums">
                    {formatMoney(plan.change)}
                  </dd>
                </>
              ) : (
                <>
                  <dt className="text-base text-muted-foreground">{t("pay.remaining")}</dt>
                  <dd className="text-right text-2xl font-semibold tabular-nums">{formatMoney(remaining)}</dd>
                </>
              )}
            </dl>

            <div className="flex flex-col gap-1">
              <Label htmlFor="pay-salesperson">{t("pay.salesperson")}</Label>
              <Input
                id="pay-salesperson"
                ref={soldByRef}
                className="h-12 w-40 text-lg tabular-nums"
                inputMode="numeric"
                autoComplete="off"
                maxLength={4}
                required
                aria-describedby="pay-salesperson-hint"
                value={soldBy}
                onChange={(e) => setSoldBy(e.target.value.replace(/\D/g, ""))}
              />
              <p id="pay-salesperson-hint" className="text-sm text-muted-foreground">
                {t("pay.salespersonHint")}
              </p>
            </div>

            {rows.map((row, i) => (
              <div key={i} className="flex items-end gap-2">
                <div className="flex flex-1 flex-col gap-1">
                  <Label htmlFor={`pay-${i}`}>
                    {t(`pay.methods.${row.method}`)} ·{" "}
                    {row.method === "cash" ? t("pay.tendered") : t("pay.amount")}
                  </Label>
                  <Input
                    id={`pay-${i}`}
                    className="h-12 text-lg tabular-nums"
                    inputMode="decimal"
                    autoComplete="off"
                    placeholder={row.method === "cash" ? formatMoney(remaining) : ""}
                    value={row.value}
                    onChange={(e) => update(i, { value: e.target.value })}
                  />
                </div>
                {row.method !== "cash" && (
                  <Input
                    aria-label={t("pay.reference")}
                    placeholder={t("pay.reference")}
                    className="h-12 w-40 text-base"
                    value={row.reference}
                    onChange={(e) => update(i, { reference: e.target.value })}
                  />
                )}
                {row.method === "cash" && (
                  <Button type="button" variant="outline" size="pos" onClick={() => update(i, { value: "" })}>
                    <Banknote aria-hidden />
                    {t("pay.exact")}
                  </Button>
                )}
                {rows.length > 1 && (
                  <WithTooltip label={t("pay.remove")}>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon-pos"
                      aria-label={t("pay.remove")}
                      onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))}
                    >
                      <Trash2 aria-hidden />
                    </Button>
                  </WithTooltip>
                )}
              </div>
            ))}

            <div className="flex flex-wrap gap-2" role="group" aria-label={t("pay.add")}>
              {METHODS.filter((m) => m !== "cash" || !hasCash).map((m) => (
                <Button key={m} type="button" variant="outline" size="pos" onClick={() => add(m)}>
                  <Plus aria-hidden />
                  {t(`pay.methods.${m}`)}
                </Button>
              ))}
            </div>

            {!plan.ok && plan.reason !== "invalid" && (
              <p className="text-base text-muted-foreground">{t(`pay.${plan.reason}`)}</p>
            )}
            {error && (
              <p role="alert" className="text-base text-destructive">
                {error}
              </p>
            )}
            <DialogFooter className="mt-2">
              <Button
                type="submit"
                size="pos-lg"
                className="w-full"
                disabled={!plan.ok || soldByNo === null || pending}
              >
                {pending ? t("pay.processing") : t("pay.complete")}
              </Button>
            </DialogFooter>
          </div>
          {touchMode && (
            <div className="hidden w-64 md:block">
              <NumericKeypad />
            </div>
          )}
        </form>
      </DialogContent>
    </Dialog>
  );
}
