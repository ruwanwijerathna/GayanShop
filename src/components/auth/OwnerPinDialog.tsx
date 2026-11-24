import { ShieldAlert } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
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
import { useVerifyOwnerPin } from "@/db/hooks";
import { playCue } from "@/lib/audio";

export interface OwnerPinDialogProps {
  open: boolean;
  title?: string;
  description?: string;
  onSuccess: () => void;
  onCancel: () => void;
}

/**
 * Dialog prompting for the Owner PIN to authorise sensitive or destructive operations
 * like deleting products with stock or clearing stock.
 */
export function OwnerPinDialog({ open, title, description, onSuccess, onCancel }: OwnerPinDialogProps) {
  const { t } = useTranslation(["products", "auth", "common"]);
  const verifyPin = useVerifyOwnerPin();
  const [pin, setPin] = useState("");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const close = () => {
    setPin("");
    setErrorMsg(null);
    verifyPin.reset();
    onCancel();
  };

  const submit = () => {
    if (!pin) return;
    setErrorMsg(null);
    verifyPin.mutate(pin, {
      onSuccess: () => {
        playCue("complete");
        setPin("");
        onSuccess();
      },
      onError: () => {
        playCue("error");
        setPin("");
        setErrorMsg(
          t("products:form.invalidOwnerPin", { defaultValue: "Invalid Owner PIN. Please try again." }),
        );
      },
    });
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()}>
      <DialogContent className="max-w-md gap-4">
        <DialogHeader className="gap-2">
          <div className="flex items-center gap-2 text-destructive">
            <ShieldAlert className="size-6 text-amber-600" />
            <DialogTitle className="text-lg">
              {title ?? t("products:form.ownerPinTitle", { defaultValue: "Owner PIN Required" })}
            </DialogTitle>
          </div>
          <DialogDescription className="text-sm text-muted-foreground">
            {description ??
              t("products:form.ownerPinPrompt", {
                defaultValue: "Enter Owner PIN to authorize this action.",
              })}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col items-center gap-3 py-2">
          <PinPad
            label={t("auth:login.pin", { defaultValue: "PIN" })}
            value={pin}
            disabled={verifyPin.isPending}
            onChange={(val) => {
              setPin(val);
              if (errorMsg) setErrorMsg(null);
            }}
            onSubmit={submit}
          />

          {errorMsg && (
            <p role="alert" className="text-sm font-medium text-destructive">
              {errorMsg}
            </p>
          )}
        </div>

        <DialogFooter className="flex justify-between sm:justify-between">
          <Button type="button" variant="outline" size="pos" onClick={close} disabled={verifyPin.isPending}>
            {t("common:actions.cancel", { defaultValue: "Cancel" })}
          </Button>
          <Button
            type="button"
            variant="destructive"
            size="pos"
            disabled={pin.length < 4 || verifyPin.isPending}
            onClick={submit}
          >
            {verifyPin.isPending
              ? t("products:form.saving", { defaultValue: "Verifying…" })
              : t("common:actions.confirm", { defaultValue: "Confirm" })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
