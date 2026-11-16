import { useState, type ReactNode } from "react";
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
import type { AllocationMethod, BatchDetail, ProductSummary } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useAutoPrintLabels } from "@/components/labels/useAutoPrintLabels";
import { useAdHocPurchase, useCreateBatch } from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";
import { parseMajor } from "@/lib/money";
import { parseOptionalMoney, parseQty } from "./batchForm";
import { METHODS } from "./IntakeWizard";
import { ProductPicker } from "./ProductPicker";
import { SelectField } from "@/components/common/SelectField";

const FIELD = "h-12 text-base";

function Field({
  id,
  label,
  hint,
  children,
}: {
  id: string;
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {hint && <p className="text-sm text-muted-foreground">{hint}</p>}
    </div>
  );
}

function FieldError({ message }: { message: string | null }) {
  return message ? (
    <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
      {message}
    </p>
  ) : null;
}

/** Start a draft batch: name, supplier and (optionally) the invoice total and extra costs. */
export function NewBatchDialog({
  open,
  onClose,
  onCreated,
}: {
  open: boolean;
  onClose: () => void;
  onCreated: (detail: BatchDetail) => void;
}) {
  const { t, i18n } = useTranslation(["batches", "common"]);
  const create = useCreateBatch();
  const [name, setName] = useState("");
  const [supplier, setSupplier] = useState("");
  const [supplierRef, setSupplierRef] = useState("");
  const [total, setTotal] = useState("");
  const [extra, setExtra] = useState("");
  const [method, setMethod] = useState<AllocationMethod>("even_per_unit");
  const [localError, setLocalError] = useState<string | null>(null);

  const serverError =
    create.error instanceof CommandError ? translateError(i18n.t, create.error.error) : null;

  const submit = () => {
    const totalCost = parseOptionalMoney(total);
    const extraCosts = parseOptionalMoney(extra);
    if (totalCost === null || extraCosts === null) {
      setLocalError(t("moneyInvalid"));
      playCue("error");
      return;
    }
    setLocalError(null);
    create.mutate(
      {
        name,
        supplier_name: supplier,
        supplier_ref: supplierRef,
        purchased_at: null,
        total_cost: totalCost,
        extra_costs: extraCosts,
        allocation_method: method,
        notes: null,
      },
      {
        onSuccess: (detail) => {
          onCreated(detail);
          onClose();
        },
        onError: () => playCue("error"),
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && !create.isPending && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("new")}</DialogTitle>
          <DialogDescription>{t("detail.draftHint")}</DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
        >
          <Field id="batch-name" label={t("fields.name")}>
            <Input
              id="batch-name"
              className={FIELD}
              value={name}
              maxLength={80}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field id="batch-supplier" label={t("fields.supplier")}>
            <Input
              id="batch-supplier"
              className={FIELD}
              value={supplier}
              maxLength={120}
              onChange={(e) => setSupplier(e.target.value)}
            />
          </Field>
          <Field id="batch-ref" label={t("fields.supplierRef")}>
            <Input
              id="batch-ref"
              className={FIELD}
              value={supplierRef}
              maxLength={120}
              onChange={(e) => setSupplierRef(e.target.value)}
            />
          </Field>
          <Field id="batch-total" label={t("fields.totalCost")} hint={t("fields.totalCostHint")}>
            <Input
              id="batch-total"
              inputMode="decimal"
              className={`${FIELD} font-mono`}
              value={total}
              onChange={(e) => setTotal(e.target.value)}
            />
          </Field>
          <Field id="batch-extra" label={t("fields.extraCosts")}>
            <Input
              id="batch-extra"
              inputMode="decimal"
              className={`${FIELD} font-mono`}
              value={extra}
              onChange={(e) => setExtra(e.target.value)}
            />
          </Field>
          <Field id="new-batch-method" label={t("wizard.method")} hint={t(`methodHints.${method}`)}>
            <SelectField
              id="new-batch-method"
              value={method}
              onValueChange={(v) => setMethod(METHODS.find((m) => m === v) ?? "manual")}
              options={METHODS.map((m) => ({ value: m, label: t(`methods.${m}`) }))}
            />
          </Field>
          <FieldError message={localError ?? serverError} />
          <DialogFooter className="gap-2">
            <Button type="button" size="pos" variant="outline" disabled={create.isPending} onClick={onClose}>
              {t("common:actions.cancel")}
            </Button>
            <Button type="submit" size="pos" disabled={create.isPending}>
              {t("common:actions.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Buy one product and receive it immediately (an ad-hoc one-line batch). */
export function QuickPurchaseDialog({
  open,
  onClose,
  onDone,
}: {
  open: boolean;
  onClose: () => void;
  onDone: (detail: BatchDetail) => void;
}) {
  const { t, i18n } = useTranslation(["batches", "common"]);
  const buy = useAdHocPurchase();
  const autoPrint = useAutoPrintLabels();
  const [product, setProduct] = useState<ProductSummary | null>(null);
  const [qty, setQty] = useState("");
  const [cost, setCost] = useState("");
  const [supplier, setSupplier] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);

  const serverError = buy.error instanceof CommandError ? translateError(i18n.t, buy.error.error) : null;

  const submit = () => {
    const units = parseQty(qty);
    const unitCost = parseMajor(cost);
    if (!product) {
      setLocalError(t("picker.label"));
    } else if (units === null) {
      setLocalError(t("qtyInvalid"));
    } else if (unitCost === null) {
      setLocalError(t("moneyInvalid"));
    } else {
      setLocalError(null);
      buy.mutate(
        { product_id: product.id, qty: units, unit_cost: unitCost, supplier_name: supplier, note: null },
        {
          onSuccess: (detail) => {
            playCue("complete");
            autoPrint([{ product_id: product.id, qty: units }]);
            onDone(detail);
            onClose();
          },
          onError: () => playCue("error"),
        },
      );
      return;
    }
    playCue("error");
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && !buy.isPending && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("quickPurchase")}</DialogTitle>
          <DialogDescription>{t("fields.product")}</DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
        >
          <ProductPicker value={product} onChange={setProduct} />
          <div className="grid grid-cols-2 gap-3">
            <Field id="quick-qty" label={t("fields.qty")}>
              <Input
                id="quick-qty"
                inputMode="numeric"
                className={`${FIELD} font-mono`}
                value={qty}
                onChange={(e) => setQty(e.target.value)}
              />
            </Field>
            <Field id="quick-cost" label={t("fields.unitCost")}>
              <Input
                id="quick-cost"
                inputMode="decimal"
                className={`${FIELD} font-mono`}
                value={cost}
                onChange={(e) => setCost(e.target.value)}
              />
            </Field>
          </div>
          <Field id="quick-supplier" label={t("fields.supplier")}>
            <Input
              id="quick-supplier"
              className={FIELD}
              value={supplier}
              maxLength={120}
              onChange={(e) => setSupplier(e.target.value)}
            />
          </Field>
          <FieldError message={localError ?? serverError} />
          <DialogFooter className="gap-2">
            <Button type="button" size="pos" variant="outline" disabled={buy.isPending} onClick={onClose}>
              {t("common:actions.cancel")}
            </Button>
            <Button type="submit" size="pos" disabled={buy.isPending}>
              {t("detail.receive")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
