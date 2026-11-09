import { PackagePlus, Plus, Trash2, Truck } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { AllocationMethod, Batch, BatchDetail, ProductSummary } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useBatch, useBatches, useReceiveBatch, useRemoveBatchItem, useSetBatchItem } from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";
import { formatMoney, parseMajor } from "@/lib/money";
import { cn } from "@/lib/utils";
import { NewBatchDialog, QuickPurchaseDialog } from "./BatchDialogs";
import { AllocationReview, ClosedNote, HeaderStep, PnlCard, ReceivedActions, Step } from "./IntakeWizard";
import { parseQty } from "./batchForm";
import { ProductPicker } from "./ProductPicker";

/** The system go-live batch shows a translated name; every other batch shows its own. */
function batchLabel(b: Batch, openingStock: string): string {
  return b.code === "OPENING-STOCK" ? openingStock : b.name;
}

/** Add a product line to a draft (or replace its quantity and cost). The cost is typed only for manual
 * allocation; by retail value uses the retail price (the product's price unless typed). */
function AddItemForm({ batchId, method }: { batchId: string; method: AllocationMethod }) {
  const { t, i18n } = useTranslation(["batches", "common"]);
  const setItem = useSetBatchItem();
  const [product, setProduct] = useState<ProductSummary | null>(null);
  const [qty, setQty] = useState("");
  const [cost, setCost] = useState("");
  const [retail, setRetail] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const manual = method === "manual";
  const serverError =
    setItem.error instanceof CommandError ? translateError(i18n.t, setItem.error.error) : null;

  const submit = () => {
    const units = parseQty(qty);
    const unitCost = manual ? parseMajor(cost) : 0;
    const retailPrice = retail.trim() === "" ? null : parseMajor(retail);
    if (!product) {
      setLocalError(t("picker.label"));
    } else if (units === null) {
      setLocalError(t("qtyInvalid"));
    } else if (unitCost === null || (retail.trim() !== "" && retailPrice === null)) {
      setLocalError(t("moneyInvalid"));
    } else {
      setLocalError(null);
      setItem.mutate(
        {
          batchId,
          input: { product_id: product.id, qty: units, unit_cost: unitCost, retail_price: retailPrice },
        },
        {
          onSuccess: () => {
            setProduct(null);
            setQty("");
            setCost("");
            setRetail("");
          },
          onError: () => playCue("error"),
        },
      );
      return;
    }
    playCue("error");
  };

  return (
    <form
      className="flex flex-col gap-3 rounded-md bg-muted p-3"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <ProductPicker value={product} onChange={setProduct} />
      <div className="flex flex-wrap items-end gap-3">
        <div className="flex w-32 flex-col gap-1.5">
          <Label htmlFor="item-qty">{t("fields.qty")}</Label>
          <Input
            id="item-qty"
            inputMode="numeric"
            className="h-12 font-mono text-base"
            value={qty}
            onChange={(e) => setQty(e.target.value)}
          />
        </div>
        {manual && (
          <div className="flex w-44 flex-col gap-1.5">
            <Label htmlFor="item-cost">{t("fields.unitCost")}</Label>
            <Input
              id="item-cost"
              inputMode="decimal"
              className="h-12 font-mono text-base"
              value={cost}
              onChange={(e) => setCost(e.target.value)}
            />
          </div>
        )}
        {method === "by_retail_value" && (
          <div className="flex w-44 flex-col gap-1.5">
            <Label htmlFor="item-retail">{t("fields.retail")}</Label>
            <Input
              id="item-retail"
              inputMode="decimal"
              className="h-12 font-mono text-base"
              placeholder={product ? formatMoney(product.price) : undefined}
              value={retail}
              onChange={(e) => setRetail(e.target.value)}
            />
          </div>
        )}
        <Button type="submit" size="pos" disabled={setItem.isPending}>
          <Plus aria-hidden />
          {t("detail.addItem")}
        </Button>
      </div>
      {(localError ?? serverError) && (
        <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
          {localError ?? serverError}
        </p>
      )}
    </form>
  );
}

function Detail({ detail }: { detail: BatchDetail }) {
  const { t, i18n } = useTranslation(["batches", "common"]);
  const receive = useReceiveBatch();
  const remove = useRemoveBatchItem();
  const [confirming, setConfirming] = useState(false);
  const { batch, items } = detail;
  const draft = batch.status === "draft";
  const failure = receive.error ?? remove.error;
  const error = failure instanceof CommandError ? translateError(i18n.t, failure.error) : null;
  const landed = batch.total_cost + batch.extra_costs;

  return (
    <section aria-labelledby="batch-title" className="flex flex-col gap-4">
      <header className="flex flex-wrap items-center gap-3">
        <h2 id="batch-title" className="flex-1 text-xl font-semibold">
          {batchLabel(batch, t("openingStock"))}
          <span className="ml-3 font-mono text-sm text-muted-foreground">{batch.code}</span>
        </h2>
        <Badge variant={draft ? "outline" : "secondary"}>{t(`status.${batch.status}`)}</Badge>
      </header>
      <dl className="grid grid-cols-2 gap-x-6 gap-y-1 text-sm xl:grid-cols-4">
        <dt className="text-muted-foreground">{t("detail.supplier")}</dt>
        <dd>{batch.supplier_name ?? "—"}</dd>
        <dt className="text-muted-foreground">{t("detail.units")}</dt>
        <dd className="font-mono">{batch.total_units}</dd>
        <dt className="text-muted-foreground">{t("detail.itemsCost")}</dt>
        <dd className="font-mono">{formatMoney(batch.items_cost)}</dd>
        {landed > 0 && (
          <>
            <dt className="text-muted-foreground">{t("detail.landed")}</dt>
            <dd className="font-mono">{formatMoney(landed)}</dd>
          </>
        )}
        {!draft && (
          <>
            <dt className="text-muted-foreground">{t("detail.variance")}</dt>
            <dd className="font-mono">{formatMoney(batch.allocation_variance)}</dd>
          </>
        )}
      </dl>
      {draft && (
        <Step n={1} title={t("wizard.details")}>
          <HeaderStep key={`${batch.id}:${batch.version}`} batch={batch} />
        </Step>
      )}
      {!draft && detail.pnl && <PnlCard pnl={detail.pnl} />}
      {draft && (
        <Step n={2} title={t("wizard.items")}>
          <AddItemForm batchId={batch.id} method={batch.allocation_method} />
        </Step>
      )}
      {items.length === 0 ? (
        <p className="text-muted-foreground">{t("detail.noItems")}</p>
      ) : (
        <table className="w-full text-left">
          <thead className="border-b border-border text-sm">
            <tr>
              <th className="py-2">{t("fields.product")}</th>
              <th className="py-2 text-right">{t("fields.qty")}</th>
              <th className="py-2 text-right">{t("fields.unitCost")}</th>
              {!draft && <th className="py-2 text-right">{t("detail.onHand")}</th>}
              {!draft && <th className="py-2 text-right">{t("detail.sold")}</th>}
              {draft && <th className="py-2" />}
            </tr>
          </thead>
          <tbody>
            {items.map((i) => (
              <tr key={i.id} className="h-12 border-b border-border">
                <td>
                  {i.product_name}
                  {i.added_after_receipt && (
                    <Badge variant="outline" className="ml-2">
                      {t("late.badge")}
                    </Badge>
                  )}
                  {i.product_sku && (
                    <span className="ml-2 font-mono text-sm text-muted-foreground">{i.product_sku}</span>
                  )}
                </td>
                <td className="text-right font-mono">{i.qty_received}</td>
                <td className="text-right font-mono">{formatMoney(i.unit_cost)}</td>
                {!draft && (
                  <td className={cn("text-right font-mono", i.on_hand < 0 && "text-destructive")}>
                    {i.on_hand}
                  </td>
                )}
                {!draft && <td className="text-right font-mono">{i.sold_qty}</td>}
                {draft && (
                  <td className="text-right">
                    <Button
                      type="button"
                      size="icon-pos"
                      variant="outline"
                      aria-label={t("detail.removeItem", { name: i.product_name })}
                      disabled={remove.isPending}
                      onClick={() => remove.mutate({ batchId: batch.id, itemId: i.id })}
                    >
                      <Trash2 aria-hidden />
                    </Button>
                  </td>
                )}
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {error && (
        <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
          {error}
        </p>
      )}
      {draft && (
        <Step n={3} title={t("wizard.review")}>
          <AllocationReview batchId={batch.id} />
        </Step>
      )}
      {batch.status === "received" && <ReceivedActions batch={batch} />}
      {batch.status === "closed" && <ClosedNote />}
      {draft &&
        (confirming ? (
          <div role="alert" className="flex flex-col gap-3 rounded-md border border-primary p-3">
            <p>{t("detail.receiveConfirm", { units: batch.total_units })}</p>
            <div className="flex gap-2">
              <Button
                type="button"
                size="pos"
                disabled={receive.isPending}
                onClick={() =>
                  receive.mutate(batch.id, {
                    onSuccess: () => {
                      playCue("complete");
                      setConfirming(false);
                    },
                    onError: () => playCue("error"),
                  })
                }
              >
                {receive.isPending ? t("detail.receiving") : t("detail.confirmReceive")}
              </Button>
              <Button type="button" size="pos" variant="outline" onClick={() => setConfirming(false)}>
                {t("common:actions.cancel")}
              </Button>
            </div>
          </div>
        ) : (
          <Button
            type="button"
            size="pos-lg"
            className="self-start"
            disabled={items.length === 0}
            onClick={() => setConfirming(true)}
          >
            <Truck aria-hidden />
            {t("detail.receive")}
          </Button>
        ))}
    </section>
  );
}

/** Batches (BATCH_MANAGE): drafts and received batches, opening stock, quick purchases. */
export function BatchesScreen() {
  const { t, i18n } = useTranslation(["batches", "common"]);
  const list = useBatches();
  const [selected, setSelected] = useState<string | null>(null);
  const [dialog, setDialog] = useState<"new" | "quick" | null>(null);
  const detail = useBatch(selected);
  const loadError = list.error instanceof CommandError ? translateError(i18n.t, list.error.error) : null;
  const batches = (list.data ?? []).filter((b) => b.code !== "ADJUSTMENTS");

  return (
    <section className="flex flex-col gap-4">
      <header className="flex flex-wrap items-center gap-3">
        <h1 className="flex-1 text-2xl font-semibold">{t("title")}</h1>
        <Button type="button" size="pos" variant="outline" onClick={() => setDialog("quick")}>
          <PackagePlus aria-hidden />
          {t("quickPurchase")}
        </Button>
        <Button type="button" size="pos" onClick={() => setDialog("new")}>
          <Plus aria-hidden />
          {t("new")}
        </Button>
      </header>
      {loadError && (
        <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
          {loadError}
        </p>
      )}
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[20rem_1fr]">
        <nav aria-label={t("title")}>
          {batches.length === 0 ? (
            <p className="text-muted-foreground">{list.isLoading ? t("common:table.loading") : t("empty")}</p>
          ) : (
            <ul className="flex flex-col gap-1">
              {batches.map((b) => (
                <li key={b.id}>
                  <button
                    type="button"
                    aria-current={selected === b.id ? "true" : undefined}
                    className={cn(
                      "flex min-h-12 w-full flex-col items-start rounded-md border border-border px-3 py-2 text-left hover:bg-accent",
                      selected === b.id && "border-primary bg-accent",
                    )}
                    onClick={() => setSelected(b.id)}
                  >
                    <span className="flex w-full items-center gap-2">
                      <span className="flex-1 font-medium">{batchLabel(b, t("openingStock"))}</span>
                      <Badge variant={b.status === "draft" ? "outline" : "secondary"}>
                        {t(`status.${b.status}`)}
                      </Badge>
                    </span>
                    <span className="font-mono text-sm text-muted-foreground">
                      {b.code} · {b.total_units} · {formatMoney(b.items_cost)}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </nav>
        <div className="rounded-lg border border-border p-4">
          {detail.data ? (
            <Detail detail={detail.data} />
          ) : (
            <p className="text-muted-foreground">{t("select")}</p>
          )}
        </div>
      </div>
      <NewBatchDialog
        open={dialog === "new"}
        onClose={() => setDialog(null)}
        onCreated={(d) => setSelected(d.batch.id)}
      />
      <QuickPurchaseDialog
        open={dialog === "quick"}
        onClose={() => setDialog(null)}
        onDone={(d) => setSelected(d.batch.id)}
      />
    </section>
  );
}
