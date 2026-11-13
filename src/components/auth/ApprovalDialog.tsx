import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "@/app/session";
import { PinPad } from "@/components/common/PinPad";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { Perm } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useLoginStaff, useRequestApproval } from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";

export interface ApprovalDialogProps {
  open: boolean;
  perm: Perm;
  /** What is being approved (order id, etc.) — the token is bound to it. */
  targetRef: string;
  onApproved: (approvalId: string) => void;
  onCancel: () => void;
}

/**
 * Supervisor step-up auth (doc §3.5): a different staff member picks themselves and enters their PIN; Rust
 * returns a single-use, 60-second token bound to (perm, target, requester) that the caller retries with.
 */
export function ApprovalDialog({ open, perm, targetRef, onApproved, onCancel }: ApprovalDialogProps) {
  const { t, i18n } = useTranslation(["auth", "app"]);
  const { session } = useSession();
  const staff = useLoginStaff(open);
  const approve = useRequestApproval();
  const [approverId, setApproverId] = useState<string | null>(null);
  const [pin, setPin] = useState("");

  const approvers = (staff.data ?? []).filter((s) => s.id !== session?.staff_id);
  const action = i18n.exists(`app:permissions.${perm}`)
    ? t(`app:permissions.${perm}`)
    : t("app:permissions.OTHER");
  const error = approve.error instanceof CommandError ? translateError(i18n.t, approve.error.error) : null;

  const close = () => {
    setApproverId(null);
    setPin("");
    approve.reset();
    onCancel();
  };

  const submit = () => {
    if (!approverId) return;
    approve.mutate(
      { perm, targetRef, approverId, pin },
      {
        onSuccess: (grant) => {
          setPin("");
          setApproverId(null);
          onApproved(grant.approval_id);
        },
        onError: () => {
          setPin("");
          playCue("error");
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("approval.title")}</DialogTitle>
          <DialogDescription>{t("approval.reason", { action })}</DialogDescription>
        </DialogHeader>
        <fieldset className="flex flex-col gap-2">
          <legend className="text-sm font-medium">{t("approval.approver")}</legend>
          <div className="grid grid-cols-2 gap-2">
            {approvers.map((s) => (
              <Button
                key={s.id}
                type="button"
                size="pos"
                variant={approverId === s.id ? "default" : "outline"}
                aria-pressed={approverId === s.id}
                onClick={() => setApproverId(s.id)}
              >
                {s.display_name}
              </Button>
            ))}
          </div>
        </fieldset>
        {approverId && (
          <PinPad
            label={t("login.pin")}
            value={pin}
            onChange={setPin}
            onSubmit={submit}
            disabled={approve.isPending}
          />
        )}
        {error && (
          <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
            {error}
          </p>
        )}
        <DialogFooter className="gap-2 sm:justify-end">
          <Button type="button" size="pos" variant="outline" onClick={close}>
            {t("approval.cancel")}
          </Button>
          <Button
            type="button"
            size="pos"
            disabled={!approverId || pin.length === 0 || approve.isPending}
            onClick={submit}
          >
            {t("approval.approve")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
