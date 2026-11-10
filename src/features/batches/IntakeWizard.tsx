// The batch intake wizard and lifecycle (doc §3.1): ① details and the allocation method, ② items, ③ the live
// allocation with its variance, then Receive; once received, the batch P&L, Reopen, a late item, and Close.
import { CheckCircle2, Lock, PackagePlus, RotateCcw } from "lucide-react";
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { AllocationMethod, Batch, BatchPnl, ProductSummary } from "@/db/bindings";
import { CommandError } from "@/db/client";
import {
  useAddLateBatchItem,
  useBatchAllocationPreview,
  useCloseBatch,
  useReopenBatch,
  useUpdateBatch,
  useWriteOffBatch,
} from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";
import { formatMoney, toMajorInput } from "@/lib/money";
import { cn } from "@/lib/utils";
import { parseOptionalMoney, parseQty } from "./batchForm";
import { ProductPicker } from "./ProductPicker";
import { SelectField } from "@/components/common/SelectField";

export const METHODS: AllocationMethod[] = ["even_per_unit", "by_retail_value", "manual"];

/** A numbered wizard section. */
export function Step({ n, title, children }: { n: number; title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3" aria-label={title}>
      <h3 className="flex items-center gap-2 text-lg font-semibold">
        <span className="flex size-7 items-center justify-center rounded-full bg-primary text-sm text-primary-foreground">
          {n}
        </span>
        {title}
      </h3>
      {children}
    </section>
  );
}

function Problem({ message }: { message: string | null }) {
  return message ? (
    <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
      {message}
    </p>
  ) : null;
}

/** ① Invoice total, extra costs and the allocation method of a draft. */
export function HeaderStep({ batch }: { batch: Batch }) {
  const { t, i18n } = useTranslation(["batches", "common"]);
  const update = useUpdateBatch();
  const [method, setMethod] = useState<AllocationMethod>(batch.allocation_method);
  const [total, setTotal] = useState(batch.total_cost > 0 ? toMajorInput(batch.total_cost) : "");
  const [extra, setExtra] = useState(batch.extra_costs > 0 ? toMajorInput(batch.extra_costs) : "");
  const [localError, setLocalError] = useState<string | null>(null);
  const serverError =
    update.error instanceof CommandError ? translateError(i18n.t, update.error.error) : null;

  const save = () => {
    const totalCost = parseOptionalMoney(total);
    const extraCosts = parseOptionalMoney(extra);
    if (totalCost === null || extraCosts === null) {
      setLocalError(t("moneyInvalid"));
      playCue("error");
      return;
    }
    setLocalError(null);
    update.mutate({
      id: batch.id,
      input: {
        name: batch.name,
        supplier_name: batch.supplier_name ?? null,
        supplier_ref: batch.supplier_ref ?? null,
        purchased_at: batch.purchased_at,
        total_cost: totalCost,
        extra_costs: extraCosts,
        allocation_method: method,
        notes: batch.notes ?? null,
      },
    });
  };

  return (
    <form
      className="grid gap-3 sm:grid-cols-3"
      onSubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <div className="flex flex-col gap-1.5 sm:col-span-3">
        <Label htmlFor="batch-method">{t("wizard.method")}</Label>
        <SelectField
          id="batch-method"
          value={method}
          disabled={batch.is_system}
          onValueChange={(v) => setMethod(METHODS.find((m) => m === v) ?? "manual")}
          aria-describedby="batch-method-hint"
          options={METHODS.map((m) => ({ value: m, label: t(`methods.${m}`) }))}
        />
        <p id="batch-method-hint" className="text-sm text-muted-foreground">
          {t(`methodHints.${method}`)}
        </p>
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="batch-edit-total">{t("fields.totalCost")}</Label>
        <Input
          id="batch-edit-total"
          inputMode="decimal"
          className="h-12 font-mono text-base"
          value={total}
          onChange={(e) => setTotal(e.target.value)}
        />
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="batch-edit-extra">{t("fields.extraCosts")}</Label>
        <Input
          id="batch-edit-extra"
          inputMode="decimal"
          className="h-12 font-mono text-base"
          value={extra}
          onChange={(e) => setExtra(e.target.value)}
        />
      </div>
      <Button type="submit" size="pos" className="self-end" disabled={update.isPending}>
        {t("wizard.saveDetails")}
      </Button>
      <div className="sm:col-span-3">
        <Problem message={localError ?? serverError} />
      </div>
    </form>
  );
}

/** ③ What Receive will fix: each layer's cost and the variance, live. */
export function AllocationReview({ batchId }: { batchId: string }) {
  const { t, i18n } = useTranslation(["batches", "common"]);
  const preview = useBatchAllocationPreview(batchId);
  const p = preview.data;
  if (!p) return null;
  const problemKey = p.problem ? `errors:VALIDATION.${p.problem}` : null;
  return (
    <div className="flex flex-col gap-3">
      <dl className="grid grid-cols-3 gap-2 text-sm">
        <div>
          <dt className="text-muted-foreground">{t("wizard.landed")}</dt>
          <dd className="font-mono text-base">{formatMoney(p.landed_cost)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t("wizard.allocated")}</dt>
          <dd className="font-mono text-base">{formatMoney(p.allocated)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t("wizard.variance")}</dt>
          <dd
            className={cn("font-mono text-base", p.variance < 0 && "text-destructive")}
            data-testid="variance"
          >
            {formatMoney(p.variance)}
          </dd>
        </div>
      </dl>
      {problemKey && <Problem message={i18n.exists(problemKey) ? i18n.t(problemKey) : p.problem} />}
      {p.lines.length > 0 && (
        <table className="w-full text-left text-sm">
          <thead className="border-b border-border">
            <tr>
              <th className="py-2">{t("fields.product")}</th>
              <th className="py-2 text-right">{t("fields.qty")}</th>
              <th className="py-2 text-right">{t("wizard.retail")}</th>
              <th className="py-2 text-right">{t("wizard.unitCost")}</th>
              <th className="py-2 text-right">{t("wizard.lineCost")}</th>
            </tr>
          </thead>
          <tbody>
            {p.lines.map((l) => (
              <tr key={l.item_id} className="h-10 border-b border-border">
                <td>{l.product_name}</td>
                <td className="text-right font-mono">{l.qty}</td>
                <td className="text-right font-mono">{formatMoney(l.retail_price)}</td>
                <td className="text-right font-mono">{formatMoney(l.unit_cost)}</td>
                <td className="text-right font-mono">{formatMoney(l.line_cost)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

/** "Did this batch make money?" */
export function PnlCard({ pnl }: { pnl: BatchPnl }) {
  const { t } = useTranslation("batches");
  const rows: [string, number, boolean?][] = [
    ["pnl.landed", pnl.landed_cost],
    ["pnl.revenue", pnl.revenue_net],
    ["pnl.cogs", pnl.cogs],
    ["pnl.writtenOff", pnl.written_off_cost],
    ["pnl.variance", pnl.allocation_variance],
    ["pnl.realised", pnl.realised_profit, true],
    ["pnl.payback", pnl.payback, true],
    ["pnl.remainingCost", pnl.remaining_value_cost],
    ["pnl.remainingRetail", pnl.remaining_value_retail],
    ["pnl.projected", pnl.projected_profit, true],
  ];
  return (
    <div className="rounded-md border border-border p-3">
      <h3 className="mb-2 text-lg font-semibold">{t("pnl.title")}</h3>
      <p className="mb-2 text-sm text-muted-foreground">
        {t("pnl.units", {
          received: pnl.units_received,
          sold: pnl.units_sold,
          left: pnl.units_remaining,
          pct: Math.floor(pnl.sell_through_bp / 100),
        })}
      </p>
      <dl className="grid grid-cols-2 gap-x-6 gap-y-1 text-sm">
        {rows.map(([key, value, signed]) => (
          <div key={key} className="contents">
            <dt className="text-muted-foreground">{t(key)}</dt>
            <dd
              className={cn(
                "text-right font-mono",
                signed && value < 0 && "text-destructive",
                signed && value > 0 && "text-success-foreground",
              )}
            >
              {formatMoney(value)}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

/** Received-batch actions: a late item (cost 0), Reopen (only before anything moved), Close (when empty). */
export function ReceivedActions({ batch }: { batch: Batch }) {
  const { t, i18n } = useTranslation(["batches", "common"]);
  const reopen = useReopenBatch();
  const close = useCloseBatch();
  const late = useAddLateBatchItem();
  const writeOff = useWriteOffBatch();
  const [confirm, setConfirm] = useState<"reopen" | "close" | "writeOff" | null>(null);
  const [product, setProduct] = useState<ProductSummary | null>(null);
  const [qty, setQty] = useState("");
  const failure = reopen.error ?? close.error ?? late.error ?? writeOff.error;
  const error = failure instanceof CommandError ? translateError(i18n.t, failure.error) : null;

  if (batch.status !== "received" || batch.is_system) return null;

  const addLate = () => {
    const units = parseQty(qty);
    if (!product || units === null) {
      playCue("error");
      return;
    }
    resetAll();
    late.mutate(
      { batchId: batch.id, input: { product_id: product.id, qty: units, retail_price: null } },
      {
        onSuccess: () => {
          setProduct(null);
          setQty("");
        },
        onError: () => playCue("error"),
      },
    );
  };

  // Only the latest action's error is shown.
  const resetAll = () => {
    reopen.reset();
    close.reset();
    late.reset();
    writeOff.reset();
  };

  const run = () => {
    resetAll();
    if (confirm === "writeOff") {
      writeOff.mutate(
        { batchId: batch.id, note: t("lifecycle.writeOffNote") },
        {
          onSuccess: () => {
            playCue("complete");
            setConfirm(null);
          },
          onError: () => {
            playCue("error");
            setConfirm(null);
          },
        },
      );
      return;
    }
    const mutation = confirm === "reopen" ? reopen : close;
    mutation.mutate(batch.id, {
      onSuccess: () => {
        playCue("complete");
        setConfirm(null);
      },
      onError: () => {
        playCue("error");
        setConfirm(null);
      },
    });
  };

  return (
    <div className="flex flex-col gap-3">
      <form
        className="flex flex-col gap-3 rounded-md bg-muted p-3"
        aria-label={t("late.title")}
        onSubmit={(e) => {
          e.preventDefault();
          addLate();
        }}
      >
        <p className="text-sm">{t("late.hint")}</p>
        <ProductPicker value={product} onChange={setProduct} />
        <div className="flex flex-wrap items-end gap-3">
          <div className="flex w-32 flex-col gap-1.5">
            <Label htmlFor="late-qty">{t("fields.qty")}</Label>
            <Input
              id="late-qty"
              inputMode="numeric"
              className="h-12 font-mono text-base"
              value={qty}
              onChange={(e) => setQty(e.target.value)}
            />
          </div>
          <Button type="submit" size="pos" variant="outline" disabled={late.isPending}>
            <PackagePlus aria-hidden />
            {t("late.add")}
          </Button>
        </div>
      </form>
      <Problem message={error} />
      {confirm ? (
        <div role="alert" className="flex flex-col gap-3 rounded-md border border-primary p-3">
          <p>
            {t(
              confirm === "reopen"
                ? "lifecycle.reopenConfirm"
                : confirm === "close"
                  ? "lifecycle.closeConfirm"
                  : "lifecycle.writeOffConfirm",
            )}
          </p>
          <div className="flex gap-2">
            <Button
              type="button"
              size="pos"
              onClick={run}
              disabled={reopen.isPending || close.isPending || writeOff.isPending}
            >
              {t(
                confirm === "reopen"
                  ? "lifecycle.confirmReopen"
                  : confirm === "close"
                    ? "lifecycle.confirmClose"
                    : "lifecycle.confirmWriteOff",
              )}
            </Button>
            <Button type="button" size="pos" variant="outline" onClick={() => setConfirm(null)}>
              {t("common:actions.cancel")}
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex flex-wrap gap-2">
          <Button type="button" size="pos" variant="outline" onClick={() => setConfirm("reopen")}>
            <RotateCcw aria-hidden />
            {t("lifecycle.reopen")}
          </Button>
          <Button type="button" size="pos" variant="outline" onClick={() => setConfirm("writeOff")}>
            {t("lifecycle.writeOff")}
          </Button>
          <Button type="button" size="pos" variant="outline" onClick={() => setConfirm("close")}>
            <Lock aria-hidden />
            {t("lifecycle.close")}
          </Button>
        </div>
      )}
    </div>
  );
}

/** A closed batch is final: say so where the actions would be. */
export function ClosedNote() {
  const { t } = useTranslation("batches");
  return (
    <p className="flex items-center gap-2 text-base text-muted-foreground">
      <CheckCircle2 aria-hidden className="size-5" />
      {t("lifecycle.closed")}
    </p>
  );
}
