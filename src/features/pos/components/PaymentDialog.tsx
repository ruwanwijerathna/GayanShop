import { Phone, X } from "lucide-react";
import { useMemo, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "@/app/session";
import { WithTooltip } from "@/components/common/WithTooltip";
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
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { PaymentLine_Deserialize as PaymentLine, PaymentMethod } from "@/db/bindings";
import { useCustomerLookup, useTouchMode } from "@/db/hooks";
import { formatMoney, parseMajor, toMajorInput, type Money } from "@/lib/money";
import { normalizeLk } from "@/lib/phone";
import { cartStore, useCart } from "../store/cart.store";
import { formatTyped } from "./CustomerPhoneField";
import { NumericKeypad } from "./NumericKeypad";

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
  // An even exchange owes nothing: send no zero-value cash row.
  if (remaining > 0 || tendered > 0)
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
  const { session } = useSession();

  const [method, setMethod] = useState<PaymentMethod>("cash");
  const [amountValue, setAmountValue] = useState("");
  const [reference, setReference] = useState("");

  const defaultStaffNo = session?.staff_no != null ? String(session.staff_no) : "";
  const [soldBy, setSoldBy] = useState(defaultStaffNo);
  const soldByRef = useRef<HTMLInputElement>(null);
  const soldByNo = parseStaffNo(soldBy);

  const rows: PaymentRow[] = useMemo(
    () => [{ method, value: amountValue, reference }],
    [method, amountValue, reference],
  );

  const plan = planPayments(due, rows);
  const remaining = Math.max(0, due - plan.paid);

  const phone = useCart((s) => s.customerPhone);
  const customerName = useCart((s) => s.customerName);
  const setCustomer = (p: string, n: string) => cartStore.getState().setCustomer(p, n);
  const e164 = normalizeLk(phone);
  const lookup = useCustomerLookup(e164);
  const found = lookup.data;
  const digits = phone.replace(/\D/g, "").length;
  let customerStatus: string | null = null;
  if (e164 && found?.customer_id) {
    customerStatus = found.display_name
      ? t("customer.known", { name: found.display_name, visit: found.visit_count + 1 })
      : t("customer.knownNoName", { visit: found.visit_count + 1 });
  } else if (e164 && found) {
    customerStatus = t("customer.new");
  } else if (digits >= 9 && !e164) {
    customerStatus = t("customer.invalid");
  }

  const handleMethodChange = (newMethod: PaymentMethod) => {
    setMethod(newMethod);
    if (newMethod === "cash") {
      setAmountValue("");
    } else {
      setAmountValue(toMajorInput(due));
    }
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (plan.ok && soldByNo !== null && !pending) onPay(plan.payments, soldByNo);
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && !pending && onCancel()}>
      <DialogContent
        className="max-w-3xl"
        onOpenAutoFocus={(e) => {
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

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <div className="flex flex-col gap-1">
                <Label htmlFor="pay-salesperson">{t("pay.salesperson")}</Label>
                <Input
                  id="pay-salesperson"
                  ref={soldByRef}
                  className="h-12 w-full text-lg tabular-nums"
                  inputMode="numeric"
                  autoComplete="off"
                  maxLength={4}
                  required
                  value={soldBy}
                  onChange={(e) => setSoldBy(e.target.value.replace(/\D/g, ""))}
                />
              </div>

              <div className="flex flex-col gap-1">
                <Label htmlFor="pay-customer-phone" className="flex items-center gap-1.5">
                  <Phone aria-hidden className="size-4" />
                  {t("customer.label")}
                </Label>
                <div className="flex gap-2">
                  <Input
                    id="pay-customer-phone"
                    className="h-12 font-mono text-base"
                    inputMode="numeric"
                    autoComplete="off"
                    placeholder={t("customer.placeholder")}
                    value={formatTyped(phone)}
                    onChange={(e) =>
                      setCustomer(e.target.value.replace(/\D/g, "").slice(0, 10), customerName)
                    }
                  />
                  {phone !== "" && (
                    <WithTooltip label={t("customer.clear")}>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-pos"
                        aria-label={t("customer.clear")}
                        onClick={() => setCustomer("", "")}
                      >
                        <X aria-hidden />
                      </Button>
                    </WithTooltip>
                  )}
                </div>
                <p
                  id="pay-customer-status"
                  role="status"
                  className="min-h-5 truncate text-sm text-muted-foreground"
                >
                  {customerStatus}
                </p>
              </div>
            </div>

            {e164 && found && !found.customer_id && (
              <div className="flex flex-col gap-1">
                <Label htmlFor="pay-customer-name">{t("customer.name")}</Label>
                <Input
                  id="pay-customer-name"
                  placeholder={t("customer.name")}
                  className="h-12 text-base"
                  maxLength={60}
                  value={customerName}
                  onChange={(e) => setCustomer(phone, e.target.value)}
                />
              </div>
            )}

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <div className="flex flex-col gap-1">
                <Label htmlFor="pay-method">{t("pay.methodLabel") ?? "Payment method"}</Label>
                <Select value={method} onValueChange={(val) => handleMethodChange(val as PaymentMethod)}>
                  <SelectTrigger id="pay-method" size="pos" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="cash">{t("pay.methods.cash")}</SelectItem>
                    <SelectItem value="card">{t("pay.methods.card")}</SelectItem>
                    <SelectItem value="bank_transfer">{t("pay.methods.bank_transfer")}</SelectItem>
                    <SelectItem value="qr">{t("pay.methods.qr")}</SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div className="flex flex-col gap-1">
                <Label htmlFor="pay-amount">{method === "cash" ? t("pay.tendered") : t("pay.amount")}</Label>
                <Input
                  id="pay-amount"
                  className="h-12 text-lg tabular-nums"
                  inputMode="decimal"
                  autoComplete="off"
                  placeholder={method === "cash" ? formatMoney(remaining) : ""}
                  value={amountValue}
                  onChange={(e) => setAmountValue(e.target.value)}
                />
              </div>
            </div>

            {method !== "cash" && (
              <div className="flex flex-col gap-1">
                <Label htmlFor="pay-reference">{t("pay.reference")}</Label>
                <Input
                  id="pay-reference"
                  aria-label={t("pay.reference")}
                  placeholder={t("pay.reference")}
                  className="h-12 text-base"
                  value={reference}
                  onChange={(e) => setReference(e.target.value)}
                />
              </div>
            )}

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
