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
import { formatMoney, parseMajor, toMajorInput, type Money } from "@/lib/money";
import { parseDiscount } from "../lib/discountInput";
import { cartStore, MAX_LINE_QTY, type CartLine } from "../store/cart.store";

const FIELD = "h-12 text-lg tabular-nums";

/**
 * F6: the selected line's qty, price (an override below list needs a reason; Rust asks for a supervisor when
 * it is beyond the cashier's limit or below the minimum price) and line discount.
 */
export function LineEditDialog({ line, onClose }: { line: CartLine; onClose: () => void }) {
  const { t } = useTranslation("pos");
  const discountRef = useRef<HTMLInputElement>(null);
  const [qty, setQty] = useState(String(line.qty));
  const [price, setPrice] = useState(toMajorInput(line.unitPrice));
  const [reason, setReason] = useState(line.overrideReason ?? "");
  const [discount, setDiscount] = useState(line.lineDiscount > 0 ? toMajorInput(line.lineDiscount) : "");

  const qtyN = /^\d{1,4}$/.test(qty.trim()) ? Number.parseInt(qty.trim(), 10) : null;
  const priceN = parseMajor(price);
  const gross = (priceN ?? 0) * (qtyN ?? 0);
  const discountN = parseDiscount(discount, gross);
  const overridden = priceN !== null && priceN < line.listPrice;
  const belowMin = priceN !== null && line.minPrice !== null && priceN < line.minPrice;
  const valid =
    qtyN !== null &&
    qtyN >= 1 &&
    qtyN <= MAX_LINE_QTY &&
    priceN !== null &&
    discountN !== null &&
    discountN <= gross &&
    (!overridden || reason.trim() !== "");

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!valid || qtyN === null || priceN === null || discountN === null) return;
    const s = cartStore.getState();
    s.setQty(line.key, qtyN);
    s.overridePrice(line.key, priceN, overridden ? reason.trim() : null);
    s.setLineDiscount(line.key, discountN);
    onClose();
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        onOpenAutoFocus={(e) => {
          e.preventDefault();
          discountRef.current?.focus();
          discountRef.current?.select();
        }}
      >
        <DialogHeader>
          <DialogTitle>{t("line.title")}</DialogTitle>
          <DialogDescription>{line.name}</DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <div className="grid grid-cols-2 gap-4">
            <div className="flex flex-col gap-2">
              <Label htmlFor="line-qty">{t("line.qty")}</Label>
              <Input
                id="line-qty"
                className={FIELD}
                inputMode="numeric"
                value={qty}
                onChange={(e) => setQty(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="line-price">{t("line.price")}</Label>
              <Input
                id="line-price"
                className={FIELD}
                inputMode="decimal"
                value={price}
                onChange={(e) => setPrice(e.target.value)}
              />
            </div>
          </div>
          {overridden && (
            <div className="flex flex-col gap-2">
              <Label htmlFor="line-reason">{t("line.reason")}</Label>
              <Input
                id="line-reason"
                className="h-12 text-base"
                maxLength={200}
                value={reason}
                onChange={(e) => setReason(e.target.value)}
                aria-invalid={reason.trim() === ""}
                aria-describedby="line-reason-hint"
              />
              <p id="line-reason-hint" className="text-sm text-muted-foreground">
                {belowMin && line.minPrice !== null
                  ? t("line.belowMin", { price: formatMoney(line.minPrice) })
                  : reason.trim() === ""
                    ? t("line.reasonRequired")
                    : null}
              </p>
            </div>
          )}
          <div className="flex flex-col gap-2">
            <Label htmlFor="line-discount">{t("line.discount")}</Label>
            <Input
              id="line-discount"
              ref={discountRef}
              className={FIELD}
              value={discount}
              placeholder="0 / 5%"
              onChange={(e) => setDiscount(e.target.value)}
              aria-invalid={discountN === null || discountN > gross}
            />
          </div>
          <DialogFooter className="flex flex-row justify-between sm:justify-between">
            <Button
              type="button"
              variant="outline"
              size="pos"
              onClick={() => {
                cartStore.getState().removeLine(line.key);
                onClose();
              }}
            >
              {t("line.remove")}
            </Button>
            <Button type="submit" size="pos" disabled={!valid}>
              {t("line.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** F7: a discount on the whole bill, apportioned across the lines by Rust (and previewed identically). */
export function BillDiscountDialog({
  current,
  base,
  onClose,
}: {
  current: Money;
  base: Money;
  onClose: () => void;
}) {
  const { t } = useTranslation("pos");
  const [value, setValue] = useState(current > 0 ? toMajorInput(current) : "");
  const amount = parseDiscount(value, base);
  const error = amount === null ? t("bill.invalid") : amount > base ? t("bill.tooBig") : null;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (amount === null || amount > base) return;
    cartStore.getState().setBillDiscount(amount);
    onClose();
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("bill.title")}</DialogTitle>
          <DialogDescription>{formatMoney(base)}</DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <div className="flex flex-col gap-2">
            <Label htmlFor="bill-discount">{t("bill.amount")}</Label>
            <Input
              id="bill-discount"
              className={FIELD}
              value={value}
              placeholder="0 / 5%"
              onChange={(e) => setValue(e.target.value)}
              aria-invalid={error !== null}
              aria-describedby="bill-discount-error"
            />
            <p id="bill-discount-error" className="min-h-5 text-sm text-destructive">
              {error}
            </p>
            {amount !== null && amount > 0 && amount <= base && (
              <p className="text-base">− {formatMoney(amount)}</p>
            )}
          </div>
          <DialogFooter className="flex flex-row justify-between sm:justify-between">
            <Button
              type="button"
              variant="outline"
              size="pos"
              onClick={() => {
                cartStore.getState().setBillDiscount(0);
                onClose();
              }}
            >
              {t("bill.clear")}
            </Button>
            <Button type="submit" size="pos" disabled={error !== null}>
              {t("bill.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
