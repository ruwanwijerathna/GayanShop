import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ApprovalDialog } from "@/components/auth/ApprovalDialog";
import type { Perm } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { translateError } from "@/i18n";

/** Must match `commands::inventory::STOCK_TARGET`. */
export const STOCK_TARGET = "stock";

/** Run a stock write; on ApprovalRequired, ask a supervisor and retry with the approval. */
export function useApprovedStock(onDone?: () => void) {
  const [pending, setPending] = useState<{ perm: Perm; retry: (id: string) => void } | null>(null);
  const run = (send: (approvalId: string | null) => Promise<unknown>) => {
    send(null).catch((e: unknown) => {
      if (e instanceof CommandError && e.error.kind === "APPROVAL_REQUIRED") {
        const perm = e.error.perm;
        setPending({ perm, retry: (id) => void send(id).catch(() => undefined) });
      }
    });
  };
  const dialog = pending ? (
    <ApprovalDialog
      open
      perm={pending.perm}
      targetRef={STOCK_TARGET}
      onApproved={(id) => {
        const p = pending;
        setPending(null);
        p.retry(id);
      }}
      onCancel={() => {
        setPending(null);
        onDone?.();
      }}
    />
  ) : null;
  return { run, dialog };
}

export function StockErrorText({ error }: { error: unknown }) {
  const { i18n } = useTranslation();
  if (!(error instanceof CommandError) || error.error.kind === "APPROVAL_REQUIRED") return null;
  return (
    <p role="alert" className="text-base text-destructive">
      {translateError(i18n.t, error.error)}
    </p>
  );
}
