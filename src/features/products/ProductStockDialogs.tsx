// Two deliberately separate stock flows, opened from a product's row menu:
//   Restock       goods arrived from a supplier -> stock goes UP by the units received.
//   Adjust stock  the shelf disagrees with the system -> recount (set the true count) or write off
//                 damaged / expired / lost units.
// Adding a brand-new product is a third flow (the "Add product" button). Rust re-checks everything and
// asks for a supervisor's approval where the role lacks it.
import { ClipboardCheck, PackagePlus, PackageX, type LucideIcon } from "lucide-react";
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { SelectField } from "@/components/common/SelectField";
import { StockErrorText, useApprovedStock } from "@/components/stock/useApprovedStock";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { DamageReason, Product } from "@/db/bindings";
import { useAdjustStock, useRecordDamage, useRecordStockCount } from "@/db/hooks";
import { playCue } from "@/lib/audio";
import { parseUnits } from "@/lib/units";

const FIELD = "h-12 text-base";
const REASONS: DamageReason[] = ["broken", "expired", "stolen", "faulty", "other"];

function StockDialogShell({
  icon: Icon,
  title,
  description,
  onClose,
  children,
}: {
  icon: LucideIcon;
  title: string;
  description: string;
  onClose: () => void;
  children: React.ReactNode;
}) {
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[85vh] flex-col gap-0 overflow-hidden rounded-2xl p-0 sm:max-w-lg">
        <DialogHeader className="border-b bg-muted/30 px-6 py-5">
          <div className="flex items-center gap-3 pr-8">
            <span
              aria-hidden
              className="flex size-11 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary ring-1 ring-primary/15"
            >
              <Icon className="size-5" />
            </span>
            <div className="min-w-0 flex-1">
              <DialogTitle className="text-xl tracking-tight">{title}</DialogTitle>
              <DialogDescription>{description}</DialogDescription>
            </div>
          </div>
        </DialogHeader>
        <div className="flex flex-col gap-4 overflow-y-auto px-6 py-5">{children}</div>
      </DialogContent>
    </Dialog>
  );
}

function OnHand({ product, after }: { product: Product; after: number | null }) {
  const { t } = useTranslation("products");
  return (
    <dl className="grid grid-cols-2 gap-3">
      <div className="rounded-xl border p-3">
        <dt className="text-sm text-muted-foreground">{t("stock.onHandNow")}</dt>
        <dd className="font-mono text-2xl" data-testid="on-hand-now">
          {product.on_hand}
        </dd>
      </div>
      <div className="rounded-xl border p-3">
        <dt className="text-sm text-muted-foreground">{t("stock.onHandAfter")}</dt>
        <dd className="font-mono text-2xl" data-testid="on-hand-after">
          {after ?? "–"}
        </dd>
      </div>
    </dl>
  );
}

function NoteField({
  id,
  label,
  value,
  onChange,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        className={FIELD}
        maxLength={200}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    </div>
  );
}

/** Restock: goods came in. Only ever adds. */
export function RestockDialog({ product, onClose }: { product: Product; onClose: () => void }) {
  const { t } = useTranslation(["products", "inventory"]);
  const adjust = useAdjustStock();
  const { run, dialog } = useApprovedStock();
  const [qty, setQty] = useState("");
  const [note, setNote] = useState("");
  const units = parseUnits(qty);
  const ok = units !== null && units > 0;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!ok || units === null) return;
    const input = {
      product_id: product.id,
      qty_delta: units,
      note: note.trim() || t("products:stock.restock.defaultNote"),
    };
    run((approvalId) =>
      adjust.mutateAsync({ input, approvalId }).then(() => {
        playCue("complete");
        toast.success(t("products:stock.restock.done", { count: units, name: product.name }));
        onClose();
      }),
    );
  };

  return (
    <StockDialogShell
      icon={PackagePlus}
      title={t("products:stock.restock.title", { name: product.name })}
      description={t("products:stock.restock.description")}
      onClose={onClose}
    >
      <form
        onSubmit={submit}
        className="flex flex-col gap-4"
        aria-label={t("products:stock.restock.title", { name: product.name })}
      >
        <OnHand product={product} after={ok && units !== null ? product.on_hand + units : null} />
        <div className="flex flex-col gap-2">
          <Label htmlFor="restock-qty">{t("products:stock.restock.qty")}</Label>
          <Input
            id="restock-qty"
            inputMode="numeric"
            className={`${FIELD} font-mono`}
            value={qty}
            onChange={(e) => setQty(e.target.value)}
            aria-invalid={qty.trim() !== "" && !ok}
          />
        </div>
        <NoteField id="restock-note" label={t("inventory:note")} value={note} onChange={setNote} />
        <StockErrorText error={adjust.error} />
        <div className="flex justify-end gap-2">
          <Button type="button" size="pos" variant="outline" onClick={onClose}>
            {t("products:stock.cancel")}
          </Button>
          <Button type="submit" size="pos" disabled={!ok || adjust.isPending}>
            {t("products:stock.restock.submit", { count: units ?? 0 })}
          </Button>
        </div>
      </form>
      {dialog}
    </StockDialogShell>
  );
}

type AdjustMode = "recount" | "writeoff";
const MODE_ICONS: Record<AdjustMode, LucideIcon> = { recount: ClipboardCheck, writeoff: PackageX };

/** Adjust stock: fix a wrong count, or write off units that are no longer sellable. Never "goods arrived". */
export function AdjustStockDialog({ product, onClose }: { product: Product; onClose: () => void }) {
  const { t } = useTranslation(["products", "inventory"]);
  const count = useRecordStockCount();
  const damage = useRecordDamage();
  const { run, dialog } = useApprovedStock();
  const [mode, setMode] = useState<AdjustMode>("recount");
  const [qty, setQty] = useState("");
  const [reason, setReason] = useState<DamageReason>("broken");
  const [note, setNote] = useState("");
  const units = parseUnits(qty);

  const after = units === null ? null : mode === "recount" ? units : product.on_hand - units;
  const noteNeeded = mode === "writeoff" && reason === "other";
  const ok = units !== null && (mode === "recount" ? true : units > 0) && (!noteNeeded || note.trim() !== "");

  const done = () => {
    playCue("complete");
    toast.success(t("products:stock.adjust.done", { name: product.name }));
    onClose();
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!ok || units === null) return;
    if (mode === "recount") {
      const input = { lines: [{ product_id: product.id, counted: units }], note: note.trim() || null };
      run((approvalId) => count.mutateAsync({ input, approvalId }).then(done));
    } else {
      const input = { product_id: product.id, qty: units, reason, note: note.trim() || null };
      run((approvalId) => damage.mutateAsync({ input, approvalId }).then(done));
    }
  };

  return (
    <StockDialogShell
      icon={ClipboardCheck}
      title={t("products:stock.adjust.title", { name: product.name })}
      description={t("products:stock.adjust.description")}
      onClose={onClose}
    >
      <form
        onSubmit={submit}
        className="flex flex-col gap-4"
        aria-label={t("products:stock.adjust.title", { name: product.name })}
      >
        <div className="grid gap-2 sm:grid-cols-2" role="group" aria-label={t("products:stock.adjust.kind")}>
          {(["recount", "writeoff"] as const).map((m) => {
            const Icon = MODE_ICONS[m];
            const active = mode === m;
            return (
              <button
                key={m}
                type="button"
                aria-pressed={active}
                onClick={() => {
                  setMode(m);
                  setQty("");
                }}
                className={`flex min-h-12 flex-col items-start gap-1 rounded-xl border p-3 text-left transition-colors focus-visible:outline-2 focus-visible:outline-ring ${
                  active ? "border-primary bg-primary/5 ring-1 ring-primary/30" : "hover:bg-muted/50"
                }`}
              >
                <span className="flex items-center gap-2 font-medium">
                  <Icon aria-hidden className="size-4" />
                  {t(`products:stock.adjust.modes.${m}.label`)}
                </span>
                <span className="text-sm text-muted-foreground">
                  {t(`products:stock.adjust.modes.${m}.hint`)}
                </span>
              </button>
            );
          })}
        </div>
        <OnHand product={product} after={after} />
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="flex flex-col gap-2">
            <Label htmlFor="adjust-qty">
              {mode === "recount"
                ? t("products:stock.adjust.counted")
                : t("products:stock.adjust.writeoffQty")}
            </Label>
            <Input
              id="adjust-qty"
              inputMode="numeric"
              className={`${FIELD} font-mono`}
              value={qty}
              onChange={(e) => setQty(e.target.value)}
              aria-invalid={qty.trim() !== "" && units === null}
            />
          </div>
          {mode === "writeoff" && (
            <div className="flex flex-col gap-2">
              <Label htmlFor="adjust-reason">{t("inventory:damage.reason")}</Label>
              <SelectField
                id="adjust-reason"
                value={reason}
                onValueChange={(v) => setReason(REASONS.find((r) => r === v) ?? "other")}
                options={REASONS.map((r) => ({ value: r, label: t(`inventory:damage.reasons.${r}`) }))}
              />
            </div>
          )}
        </div>
        <NoteField
          id="adjust-note"
          label={noteNeeded ? t("inventory:noteRequired") : t("inventory:note")}
          value={note}
          onChange={setNote}
        />
        <StockErrorText error={mode === "recount" ? count.error : damage.error} />
        <div className="flex justify-end gap-2">
          <Button type="button" size="pos" variant="outline" onClick={onClose}>
            {t("products:stock.cancel")}
          </Button>
          <Button type="submit" size="pos" disabled={!ok || count.isPending || damage.isPending}>
            {mode === "recount" ? t("products:stock.adjust.saveCount") : t("products:stock.adjust.writeOff")}
          </Button>
        </div>
      </form>
      {dialog}
    </StockDialogShell>
  );
}
