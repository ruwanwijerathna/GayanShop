import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "@/app/session";
import { useAutoPrintLabels } from "@/components/labels/useAutoPrintLabels";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { Category, Product } from "@/db/bindings";
import { CommandError } from "@/db/client";
import {
  useAdjustStock,
  useAppStatus,
  useDeleteProduct,
  useProducts,
  useSaveCategory,
  useSaveProduct,
} from "@/db/hooks";
import { translateError } from "@/i18n";
import { Plus, Sparkles } from "lucide-react";
import { playCue } from "@/lib/audio";
import { hasPerm } from "@/lib/permissions";
import { categoryPath } from "./categories";
import { deviceDigit, nextBarcode, nextSku } from "./codes";
import { emptyForm, fromProduct, toInput, type MoneyField, type ProductFormState } from "./productForm";
import { CreatableCombobox } from "@/components/common/CreatableCombobox";
import { Textarea } from "@/components/ui/textarea";
import { WithTooltip } from "@/components/common/WithTooltip";

const FIELD = "h-12 text-base";

export interface ProductFormPanelProps {
  /** `null` = create. */
  product: Product | null;
  categories: Category[];
  /** Leave edit mode (or finish a save): the panel goes back to an empty "new product" form. */
  onDone: () => void;
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

/**
 * Create or edit a product (PRODUCT_MANAGE), in the panel beside the product list. The parent re-creates
 * it per product via `key`, so every product (and every fresh "new") starts from its own clean form.
 */
export function ProductFormPanel({ product, categories, onDone, onSaved }: ProductFormPanelProps) {
  const { t, i18n } = useTranslation(["products", "common"]);
  const { session } = useSession();
  const canViewCost = hasPerm(session, "VIEW_COST");
  const qc = useQueryClient();
  const save = useSaveProduct();
  const remove = useDeleteProduct();
  const adjust = useAdjustStock();
  const autoPrint = useAutoPrintLabels();
  const saveCategory = useSaveCategory();
  const products = useProducts();
  const status = useAppStatus();
  const [created, setCreated] = useState<Category[]>([]);
  // Products are sold whenever they are in stock, so saving always leaves a product active.
  const [form, setForm] = useState<ProductFormState>(() => ({
    ...(product ? fromProduct(product) : emptyForm()),
    isActive: true,
  }));
  const barcodeInput = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!product) barcodeInput.current?.focus();
  }, [product]);
  const [moneyError, setMoneyError] = useState<MoneyField | null>(null);
  const [quantityError, setQuantityError] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const set = <K extends keyof ProductFormState>(key: K, value: ProductFormState[K]) =>
    setForm((f) => ({ ...f, [key]: value }));

  const failure = save.error ?? remove.error ?? adjust.error ?? saveCategory.error;
  const serverError = failure instanceof CommandError ? translateError(i18n.t, failure.error) : null;
  const busy = save.isPending || remove.isPending || adjust.isPending;

  const categoryOptions = [
    ...categories,
    ...created.filter((c) => !categories.some((x) => x.id === c.id)),
  ].map((c) => ({ value: c.id, label: categoryPath(c, [...categories, ...created]) }));

  const createCategory = async (name: string): Promise<string | null> => {
    try {
      const made = await saveCategory.mutateAsync({ id: null, input: { name, parent_id: null } });
      setCreated((list) => [...list, made]);
      playCue("complete");
      return made.id;
    } catch {
      playCue("error");
      return null;
    }
  };

  const generate = (which: "sku" | "barcode") => {
    const digit = deviceDigit(status.data?.device_code);
    const list = products.data ?? [];
    const next = which === "sku" ? nextSku(digit, list, form.sku) : nextBarcode(digit, list, form.barcode);
    set(which, next);
    set(which === "sku" ? "skuMode" : "barcodeMode", "manual");
  };

  const submit = async () => {
    const qtyText = product ? "" : form.quantity.trim();
    if (qtyText !== "" && !/^\d{1,7}$/.test(qtyText)) {
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

    const qtyNumber = qtyText !== "" ? Number.parseInt(qtyText, 10) : 0;
    try {
      const saved = await save.mutateAsync({ id: product?.id ?? null, input: built.input });
      if (!product && qtyNumber > 0) {
        try {
          await adjust.mutateAsync({
            input: {
              product_id: saved.id,
              qty_delta: qtyNumber,
              note: form.note.trim() || "Opening stock",
            },
            approvalId: null,
          });
          // The opening stock is in: one sticker per unit when "Print labels when stock arrives" is on.
          autoPrint([{ product_id: saved.id, qty: qtyNumber }]);
        } catch {
          // A refused stock entry does not roll back the saved product.
        }
      }
      await qc.invalidateQueries({ queryKey: ["catalog"] });
      playCue("complete");
      onSaved?.(!product && qtyNumber > 0 ? { ...saved, on_hand: qtyNumber } : saved);
      onDone();
    } catch {
      playCue("error");
    }
  };

  const doDelete = () => {
    if (!product) return;
    remove.mutate(product.id, { onSuccess: onDone, onError: () => playCue("error") });
  };

  const codeField = (which: "sku" | "barcode") => {
    const modeKey = which === "sku" ? "skuMode" : "barcodeMode";
    const fieldLabel = t(`form.${which}`);
    const generateLabel = which === "sku" ? t("form.generateSku") : t("form.generateBarcode");

    return (
      <Field id={`product-${which}`} label={fieldLabel}>
        <div className="flex gap-2">
          <Input
            id={`product-${which}`}
            ref={which === "barcode" ? barcodeInput : undefined}
            className={`${FIELD} font-mono min-w-0 flex-1`}
            value={form[which]}
            maxLength={which === "sku" ? 16 : 32}
            autoComplete="off"
            placeholder={which === "sku" ? t("form.skuPlaceholder") : t("form.barcodePlaceholder")}
            aria-describedby={`product-${which}-hint`}
            onChange={(e) => {
              const val = e.target.value;
              set(which, val);
              set(modeKey, val.trim() === "" ? "none" : "manual");
            }}
          />
          <WithTooltip label={generateLabel}>
            <Button
              type="button"
              size="icon-pos"
              variant="outline"
              aria-label={generateLabel}
              disabled={status.isPending}
              onClick={() => generate(which)}
            >
              <Sparkles className="size-5" />
            </Button>
          </WithTooltip>
        </div>
        <p id={`product-${which}-hint`} className="text-sm text-muted-foreground">
          {which === "sku" ? t("form.skuHint") : t("form.barcodeHint")}
        </p>
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

  const stocked = product !== null && product.track_stock && product.on_hand !== 0;

  return (
    <Card className="gap-0 py-0 shadow-sm lg:sticky lg:top-4">
      <div className="flex items-start justify-between gap-2 border-b px-4 py-3">
        <div className="min-w-0">
          <h3 className="text-base font-semibold">{product ? t("edit") : t("add")}</h3>
          <p className="truncate text-sm text-muted-foreground">{product?.name ?? t("form.addHint")}</p>
        </div>
        {product && (
          <Button type="button" size="pos" variant="outline" className="shrink-0" onClick={onDone}>
            <Plus aria-hidden />
            {t("form.newProduct")}
          </Button>
        )}
      </div>
      <form
        aria-label={product ? t("edit") : t("add")}
        className="flex flex-col gap-4 p-4"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        {codeField("barcode")}
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
        {codeField("sku")}
        {canViewCost && money("cost", "product-cost", t("form.cost"))}
        {money("price", "product-price", t("form.price"))}
        {product ? (
          <p className="rounded-md border border-dashed bg-muted/40 px-3 py-2 text-sm text-muted-foreground">
            {t("form.stockElsewhere", { count: product.on_hand })}
          </p>
        ) : (
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
        )}
        <Field id="product-category" label={t("form.category")}>
          <CreatableCombobox
            id="product-category"
            value={form.categoryId}
            options={categoryOptions}
            onValueChange={(v) => set("categoryId", v)}
            onCreate={createCategory}
            emptyLabel={t("form.noCategory")}
            createLabel={(typed) => t("form.createCategory", { name: typed })}
            noMatchLabel={t("form.noCategoryMatch")}
            placeholder={t("form.categoryPlaceholder")}
          />
        </Field>
        {!product && /^\d{1,7}$/.test(form.quantity.trim()) && Number.parseInt(form.quantity, 10) > 0 && (
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="product-note">{t("form.note")}</Label>
            <Textarea
              id="product-note"
              rows={2}
              className="resize-y text-base"
              value={form.note}
              placeholder={t("form.notePlaceholder")}
              maxLength={200}
              onChange={(e) => set("note", e.target.value)}
            />
          </div>
        )}
        {serverError && (
          <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
            {serverError}
          </p>
        )}
        {confirmDelete && product && (
          <div role="alert" className="flex flex-col gap-3 rounded-md border border-destructive p-3">
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
        <Button type="submit" size="pos-lg" disabled={busy}>
          {save.isPending ? t("form.saving") : t("common:actions.save")}
        </Button>
        {product && !confirmDelete && (
          <div className="flex flex-col gap-1.5">
            <Button
              type="button"
              size="pos"
              variant="outline"
              disabled={busy || stocked}
              aria-describedby={stocked ? "product-delete-hint" : undefined}
              onClick={() => setConfirmDelete(true)}
            >
              {t("form.delete")}
            </Button>
            {stocked && (
              <p id="product-delete-hint" className="text-sm text-muted-foreground">
                {t("form.deleteNeedsZero", { count: product.on_hand })}
              </p>
            )}
          </div>
        )}
      </form>
    </Card>
  );
}
