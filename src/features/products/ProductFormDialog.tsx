import { Plus, X } from "lucide-react";
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "@/app/session";
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
import type { Category, Product } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useAdjustStock, useDeleteProduct, useSaveProduct } from "@/db/hooks";
import { translateError } from "@/i18n";
import { Sparkles } from "lucide-react";
import { playCue } from "@/lib/audio";
import { hasPerm } from "@/lib/permissions";
import { categoryPath } from "./CategoryPanel";
import { emptyForm, fromProduct, toInput, type MoneyField, type ProductFormState } from "./productForm";
import { SelectField } from "@/components/common/SelectField";
import { Switch } from "@/components/ui/switch";
import { WithTooltip } from "@/components/common/WithTooltip";

const FIELD = "h-12 text-base";

export interface ProductFormDialogProps {
  /** `null` = create. */
  product: Product | null;
  categories: Category[];
  open: boolean;
  onClose: () => void;
  onSaved?: (product: Product) => void;
}

function Field({ id, label, children }: { id: string; label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children}
    </div>
  );
}

/** Create or edit a product (PRODUCT_MANAGE). The form is re-created per product via `key`. */
export function ProductFormDialog(props: ProductFormDialogProps) {
  return props.open ? <ProductForm key={props.product?.id ?? "new"} {...props} /> : null;
}

function ProductForm({ product, categories, open, onClose, onSaved }: ProductFormDialogProps) {
  const { t, i18n } = useTranslation(["products", "common"]);
  const { session } = useSession();
  const canViewCost = hasPerm(session, "VIEW_COST");
  const save = useSaveProduct();
  const remove = useDeleteProduct();
  const adjust = useAdjustStock();
  const [form, setForm] = useState<ProductFormState>(() => (product ? fromProduct(product) : emptyForm()));
  const [moneyError, setMoneyError] = useState<MoneyField | null>(null);
  const [quantityError, setQuantityError] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const set = <K extends keyof ProductFormState>(key: K, value: ProductFormState[K]) =>
    setForm((f) => ({ ...f, [key]: value }));

  const failure = save.error ?? remove.error ?? adjust.error;
  const serverError = failure instanceof CommandError ? translateError(i18n.t, failure.error) : null;
  const busy = save.isPending || remove.isPending || adjust.isPending;

  const submit = () => {
    const qtyText = form.quantity.trim();
    if (qtyText !== "" && (!/^\d{1,7}$/.test(qtyText) || Number.parseInt(qtyText, 10) < 0)) {
      setQuantityError(true);
      playCue("error");
      return;
    }
    setQuantityError(false);

    const built = toInput(form, canViewCost);
    if (!built.ok) {
      setMoneyError(built.field);
      playCue("error");
      return;
    }
    setMoneyError(null);
    save.mutate(
      { id: product?.id ?? null, input: built.input },
      {
        onSuccess: async (saved) => {
          if (qtyText !== "") {
            const targetQty = Number.parseInt(qtyText, 10);
            const currentOnHand = product?.on_hand ?? 0;
            const delta = !product ? targetQty : targetQty - currentOnHand;
            if (delta !== 0) {
              const noteText = form.note.trim() || (!product ? "Initial stock" : "Stock adjustment");
              try {
                await adjust.mutateAsync({
                  input: {
                    product_id: saved.id,
                    qty_delta: delta,
                    note: noteText,
                  },
                  approvalId: null,
                });
              } catch {
                // Stock adjust refusal or error does not roll back the product save
              }
            }
          }
          playCue("complete");
          onSaved?.(saved);
          onClose();
        },
        onError: () => playCue("error"),
      },
    );
  };

  const doDelete = () => {
    if (!product) return;
    remove.mutate(product.id, { onSuccess: onClose, onError: () => playCue("error") });
  };

  const codeField = (which: "sku" | "barcode") => {
    const modeKey = which === "sku" ? "skuMode" : "barcodeMode";
    const mode = form[modeKey];
    const fieldLabel = t(`form.${which}`);
    const isGenerate = mode === "generate";
    const generateLabel = which === "sku" ? t("form.generateSku") : t("form.generateBarcode");
    const cancelLabel = which === "sku" ? t("form.cancelGenerateSku") : t("form.cancelGenerateBarcode");
    const placeholder = isGenerate
      ? t("form.codeAutoGenerated")
      : which === "sku"
        ? t("form.skuPlaceholder")
        : t("form.barcodePlaceholder");

    return (
      <Field id={`product-${which}`} label={fieldLabel}>
        <div className="flex gap-2">
          <Input
            id={`product-${which}`}
            aria-label={fieldLabel}
            className={`${FIELD} font-mono flex-1`}
            value={isGenerate ? "" : form[which]}
            maxLength={which === "sku" ? 16 : 32}
            autoComplete="off"
            placeholder={placeholder}
            onChange={(e) => {
              const val = e.target.value;
              set(which, val);
              set(modeKey, val.trim() === "" ? "none" : "manual");
            }}
          />
          <WithTooltip label={isGenerate ? cancelLabel : generateLabel}>
            <Button
              type="button"
              size="icon-pos"
              variant={isGenerate ? "default" : "outline"}
              aria-label={isGenerate ? cancelLabel : generateLabel}
              aria-pressed={isGenerate}
              onClick={() => {
                if (isGenerate) {
                  set(modeKey, form[which].trim() === "" ? "none" : "manual");
                } else {
                  set(modeKey, "generate");
                  set(which, "");
                }
              }}
            >
              <Sparkles className="size-5" />
            </Button>
          </WithTooltip>
        </div>
        {isGenerate && <p className="text-xs text-muted-foreground">{t("form.codeGenerateHint")}</p>}
      </Field>
    );
  };

  const money = (field: MoneyField, id: string, label: string) => (
    <Field id={id} label={label}>
      <Input
        id={id}
        inputMode="decimal"
        className={`${FIELD} font-mono`}
        value={form[field]}
        aria-invalid={moneyError === field}
        aria-describedby={moneyError === field ? `${id}-error` : undefined}
        onChange={(e) => set(field, e.target.value)}
      />
      {moneyError === field && (
        <p id={`${id}-error`} className="text-sm text-destructive">
          {t("form.moneyInvalid")}
        </p>
      )}
    </Field>
  );

  return (
    <Dialog open={open} onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{product ? t("edit") : t("add")}</DialogTitle>
          <DialogDescription>{product?.name ?? t("form.codeGenerateHint")}</DialogDescription>
        </DialogHeader>
        <form
          className="grid grid-cols-1 gap-4 xl:grid-cols-2"
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
        >
          <Field id="product-name" label={t("form.name")}>
            <Input
              id="product-name"
              className={FIELD}
              value={form.name}
              placeholder={t("form.namePlaceholder")}
              maxLength={80}
              autoComplete="off"
              onChange={(e) => set("name", e.target.value)}
            />
          </Field>
          <Field id="product-category" label={t("form.category")}>
            <SelectField
              id="product-category"
              value={form.categoryId}
              onValueChange={(v) => set("categoryId", v)}
              options={[
                { value: "", label: t("form.noCategory") },
                ...categories.map((c) => ({ value: c.id, label: categoryPath(c, categories) })),
              ]}
            />
          </Field>
          {money("price", "product-price", t("form.price"))}
          {canViewCost && money("cost", "product-cost", t("form.cost"))}
          <Field id="product-quantity" label={t("form.quantity")}>
            <Input
              id="product-quantity"
              inputMode="numeric"
              className={`${FIELD} font-mono`}
              value={form.quantity}
              placeholder={t("form.quantityPlaceholder")}
              aria-invalid={quantityError}
              aria-describedby={quantityError ? "product-quantity-error" : undefined}
              onChange={(e) => {
                set("quantity", e.target.value);
                if (quantityError) setQuantityError(false);
              }}
            />
            {quantityError && (
              <p id="product-quantity-error" className="text-sm text-destructive">
                {t("form.quantityInvalid")}
              </p>
            )}
          </Field>
          <Field id="product-note" label={t("form.note")}>
            <Input
              id="product-note"
              className={FIELD}
              value={form.note}
              placeholder={t("form.notePlaceholder")}
              maxLength={200}
              onChange={(e) => set("note", e.target.value)}
            />
          </Field>
          <div className="xl:col-span-2 grid grid-cols-1 gap-4 xl:grid-cols-2">
            {codeField("sku")}
            {codeField("barcode")}
          </div>
          <fieldset className="flex flex-col gap-2 xl:col-span-2">
            <legend className="mb-1.5 text-sm font-medium">{t("form.attributes")}</legend>
            {form.attributes.map((row, i) => (
              <div key={i} className="flex gap-2">
                <Input
                  aria-label={t("form.attrName")}
                  className={FIELD}
                  value={row.key}
                  maxLength={20}
                  onChange={(e) =>
                    set(
                      "attributes",
                      form.attributes.map((r, j) => (j === i ? { ...r, key: e.target.value } : r)),
                    )
                  }
                />
                <Input
                  aria-label={t("form.attrValue")}
                  className={FIELD}
                  value={row.value}
                  maxLength={40}
                  onChange={(e) =>
                    set(
                      "attributes",
                      form.attributes.map((r, j) => (j === i ? { ...r, value: e.target.value } : r)),
                    )
                  }
                />
                <WithTooltip label={t("form.removeAttribute", { name: row.key })}>
                  <Button
                    type="button"
                    size="icon-pos"
                    variant="outline"
                    aria-label={t("form.removeAttribute", { name: row.key })}
                    onClick={() =>
                      set(
                        "attributes",
                        form.attributes.filter((_, j) => j !== i),
                      )
                    }
                  >
                    <X aria-hidden />
                  </Button>
                </WithTooltip>
              </div>
            ))}
            {form.attributes.length < 10 && (
              <Button
                type="button"
                size="pos"
                variant="outline"
                className="self-start"
                onClick={() => set("attributes", [...form.attributes, { key: "", value: "" }])}
              >
                <Plus aria-hidden />
                {t("form.addAttribute")}
              </Button>
            )}
          </fieldset>
          <div className="flex items-center justify-between gap-4 rounded-lg border p-3.5 xl:col-span-2">
            <div className="flex flex-col gap-0.5">
              <Label htmlFor="product-active" className="cursor-pointer text-base font-medium">
                {t("form.active")}
              </Label>
              <span className="text-sm text-muted-foreground">
                {form.isActive ? t("form.activeHint") : t("form.inactiveHint")}
              </span>
            </div>
            <Switch
              id="product-active"
              size="pos"
              checked={form.isActive}
              onCheckedChange={(v) => set("isActive", v)}
            />
          </div>
          {serverError && (
            <p
              role="alert"
              className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground xl:col-span-2"
            >
              {serverError}
            </p>
          )}
          {confirmDelete && product && (
            <div
              role="alert"
              className="flex flex-col gap-3 rounded-md border border-destructive p-3 xl:col-span-2"
            >
              <p>{t("form.deleteConfirm", { name: product.name })}</p>
              <div className="flex gap-2">
                <Button type="button" size="pos" variant="destructive" disabled={busy} onClick={doDelete}>
                  {t("form.confirmDelete")}
                </Button>
                <Button type="button" size="pos" variant="outline" onClick={() => setConfirmDelete(false)}>
                  {t("common:actions.cancel")}
                </Button>
              </div>
            </div>
          )}
          <DialogFooter className="gap-2 xl:col-span-2">
            {product && !confirmDelete && (
              <Button
                type="button"
                size="pos"
                variant="outline"
                className="mr-auto"
                disabled={busy}
                onClick={() => setConfirmDelete(true)}
              >
                {t("form.delete")}
              </Button>
            )}
            <Button type="button" size="pos" variant="outline" disabled={busy} onClick={onClose}>
              {t("common:actions.cancel")}
            </Button>
            <Button type="submit" size="pos" disabled={busy}>
              {save.isPending ? t("form.saving") : t("common:actions.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
