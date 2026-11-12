import * as React from "react";
import { KeyRound } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { Role, StaffMember_Serialize as StaffMember } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useResetStaffPin } from "@/db/hooks";
import { translateError } from "@/i18n";
import { cn } from "@/lib/utils";
import { minPin } from "./permissions";

export function PinDialog({
  member,
  roles,
  onClose,
}: {
  member: StaffMember;
  roles: Role[];
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation("staff");
  const reset = useResetStaffPin();
  const [pin, setPin] = React.useState("");

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    reset.mutate(
      { id: member.id, pin },
      {
        onSuccess: () => {
          toast.success(t("pin.done", { name: member.display_name }));
          onClose();
        },
      },
    );
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="gap-0 overflow-hidden rounded-2xl p-0 sm:max-w-md">
        <DialogHeader className="border-b bg-muted/30 px-6 py-5">
          <div className="flex items-center gap-3">
            <span
              aria-hidden
              className="flex size-11 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary ring-1 ring-primary/15"
            >
              <KeyRound className="size-5" />
            </span>
            <DialogTitle className="text-xl tracking-tight">
              {t("pin.title", { name: member.display_name })}
            </DialogTitle>
          </div>
          <DialogDescription className="sr-only">{t("resetPin")}</DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex flex-col gap-4 px-6 py-6">
          <Label htmlFor="new-pin">{t("pin.label", { min: minPin(roles, member.role_id) })}</Label>
          <Input
            id="new-pin"
            className={cn("h-14 text-center text-2xl tracking-[0.5em]")}
            type="password"
            inputMode="numeric"
            autoComplete="new-password"
            maxLength={8}
            value={pin}
            onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
          />
          {reset.error instanceof CommandError && (
            <p
              role="alert"
              className="rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 text-base text-destructive motion-safe:animate-in motion-safe:fade-in-0 motion-safe:slide-in-from-top-1"
            >
              {translateError(i18n.t, reset.error.error)}
            </p>
          )}
          <div className="flex justify-end gap-2 pt-2">
            <Button type="button" variant="ghost" size="pos" className="rounded-xl" onClick={onClose}>
              {t("form.cancel")}
            </Button>
            <Button
              type="submit"
              size="pos"
              className="rounded-xl px-6 shadow-sm"
              disabled={reset.isPending || pin.length === 0}
            >
              {t("pin.save")}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
