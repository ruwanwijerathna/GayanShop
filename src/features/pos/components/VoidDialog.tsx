import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ApprovalDialog } from "@/components/auth/ApprovalDialog";
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
import type { VoidReason } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useSessionOrders, useVoidOrder } from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";
import { formatMoney } from "@/lib/money";
import { cn } from "@/lib/utils";
import { SelectField } from "@/components/common/SelectField";

const REASONS: VoidReason[] = [
  "WRONG_ITEM",
  "WRONG_PRICE",
  "WRONG_QTY",
  "DUPLICATE_SCAN",
  "CUSTOMER_CANCELLED",
  "OTHER",
];

/**
 * F10 (doc §3.3): the open session's sales on this counter. All sales are final; a void cancels a whole sale,
 * with a reason (a note for Other). A cashier gets a supervisor's approval bound to that order and retries.
 */
export function VoidDialog({ onClose, preselect }: { onClose: () => void; preselect?: string | null }) {
  const { t, i18n } = useTranslation("pos");
  const orders = useSessionOrders(true);
  const voidOrder = useVoidOrder();
  const [selected, setSelected] = useState<string | null>(preselect ?? null);
  const [reason, setReason] = useState<VoidReason>("WRONG_ITEM");
  const [note, setNote] = useState("");
  const [approving, setApproving] = useState(false);

  const send = (approvalId: string | null) => {
    if (!selected) return;
    voidOrder.mutate(
      { order_id: selected, reason, note: note.trim() || null, approval_id: approvalId },
      {
        onSuccess: (r) => {
          playCue("complete");
          toast.success(
            r.cash_returned > 0
              ? t("void.done", { receipt: r.receipt_no, cash: formatMoney(r.cash_returned) })
              : t("void.doneNoCash", { receipt: r.receipt_no }),
          );
          onClose();
        },
        onError: (e) => {
          if (e instanceof CommandError && e.needsApproval) {
            playCue("approval");
            setApproving(true);
          } else {
            playCue("error");
          }
        },
      },
    );
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    send(null);
  };

  const error =
    voidOrder.error instanceof CommandError && !voidOrder.error.needsApproval
      ? translateError(i18n.t, voidOrder.error.error)
      : null;
  const list = orders.data ?? [];
  const noteMissing = reason === "OTHER" && note.trim() === "";

  return (
    <>
      <Dialog open={!approving} onOpenChange={(o) => !o && onClose()}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("void.title")}</DialogTitle>
            <DialogDescription>{t("void.help")}</DialogDescription>
          </DialogHeader>
          <form onSubmit={submit} className="flex flex-col gap-4">
            {list.length === 0 ? (
              <p className="text-base text-muted-foreground">{t("void.empty")}</p>
            ) : (
              <ul className="flex max-h-72 flex-col gap-2 overflow-y-auto" aria-label={t("void.title")}>
                {list.map((o) => {
                  const done = o.status === "voided";
                  return (
                    <li key={o.order_id}>
                      <button
                        type="button"
                        disabled={done}
                        aria-pressed={selected === o.order_id}
                        onClick={() => setSelected(o.order_id)}
                        className={cn(
                          "flex min-h-12 w-full items-center gap-3 rounded-md border px-3 text-left text-base disabled:opacity-60",
                          selected === o.order_id && "border-primary bg-accent",
                        )}
                      >
                        <span className="font-mono">{o.receipt_no}</span>
                        <span className="flex-1 truncate text-muted-foreground">
                          {o.staff_name} · {new Date(o.completed_at).toLocaleTimeString()}
                        </span>
                        <span className="tabular-nums">{formatMoney(o.grand_total)}</span>
                        {done && <span className="text-sm">{t("void.voided")}</span>}
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="flex flex-col gap-2">
                <Label htmlFor="void-reason">{t("void.reason")}</Label>
                <SelectField
                  id="void-reason"
                  value={reason}
                  onValueChange={(v) => setReason(REASONS.find((r) => r === v) ?? "WRONG_ITEM")}
                  options={REASONS.map((r) => ({ value: r, label: t(`void.reasons.${r}`) }))}
                />
              </div>
              <div className="flex flex-col gap-2">
                <Label htmlFor="void-note">{t("void.note")}</Label>
                <Input
                  id="void-note"
                  className="h-12 text-base"
                  maxLength={200}
                  value={note}
                  onChange={(e) => setNote(e.target.value)}
                  aria-invalid={noteMissing}
                />
              </div>
            </div>
            {error && (
              <p role="alert" className="text-base text-destructive">
                {error}
              </p>
            )}
            <DialogFooter className="gap-2 sm:justify-end">
              <Button type="button" variant="outline" size="pos" onClick={onClose}>
                {i18n.t("common:actions.cancel", { defaultValue: "Cancel" })}
              </Button>
              <Button
                type="submit"
                variant="destructive"
                size="pos"
                disabled={!selected || noteMissing || voidOrder.isPending}
              >
                {t("void.submit")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <ApprovalDialog
        open={approving}
        perm="VOID_ORDER"
        targetRef={selected ?? ""}
        onApproved={(id) => {
          setApproving(false);
          send(id);
        }}
        onCancel={() => setApproving(false)}
      />
    </>
  );
}
