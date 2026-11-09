import { Banknote, DoorOpen, LockKeyhole, Minus, Plus } from "lucide-react";
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ApprovalDialog } from "@/components/auth/ApprovalDialog";
import { Can } from "@/components/auth/Can";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { CashSession, DrawerMovementKind, Perm, ZReport } from "@/db/bindings";
import { CommandError } from "@/db/client";
import {
  useCloseCashSession,
  useCurrentCashSession,
  useDrawerMovement,
  useNoSale,
  useOpenCashSession,
} from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";
import { formatMoney, parseMajor } from "@/lib/money";
import { ZReportView } from "./ZReportView";

/** Must match `commands::cash::DRAWER_TARGET`: the approval is bound to it. */
export const DRAWER_TARGET = "cash_drawer";

/** Sri Lankan notes and coins, in cents (mirrors `cash::LKR_DENOMINATIONS`). */
export const DENOMINATIONS = [500_000, 200_000, 100_000, 50_000, 10_000, 5_000, 2_000, 1_000, 500, 200, 100];

const FIELD = "h-12 text-base";

function Field({
  id,
  label,
  hint,
  children,
}: {
  id: string;
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {hint && <p className="text-sm text-muted-foreground">{hint}</p>}
    </div>
  );
}

function Alert({ message }: { message: string | null }) {
  return message ? (
    <p role="alert" className="rounded-md bg-destructive px-3 py-2 text-destructive-foreground">
      {message}
    </p>
  ) : null;
}

/** Σ value × count of a denomination count (cents). */
export function denominationTotal(counts: Record<number, string>): number | null {
  let total = 0;
  for (const value of DENOMINATIONS) {
    const raw = (counts[value] ?? "").trim();
    if (raw === "") continue;
    if (!/^\d{1,6}$/.test(raw)) return null;
    total += value * Number.parseInt(raw, 10);
  }
  return total;
}

function OpenSessionForm() {
  const { t, i18n } = useTranslation(["cash", "common"]);
  const openSession = useOpenCashSession();
  const [float, setFloat] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const serverError =
    openSession.error instanceof CommandError ? translateError(i18n.t, openSession.error.error) : null;
  return (
    <form
      aria-labelledby="open-title"
      className="flex max-w-md flex-col gap-4 rounded-lg border border-border p-4"
      onSubmit={(e) => {
        e.preventDefault();
        const cents = float.trim() === "" ? 0 : parseMajor(float);
        if (cents === null) {
          setLocalError(t("moneyInvalid"));
          playCue("error");
          return;
        }
        setLocalError(null);
        openSession.mutate(cents, { onError: () => playCue("error") });
      }}
    >
      <h2 id="open-title" className="text-xl font-semibold">
        {t("open.title")}
      </h2>
      <p className="text-muted-foreground">{t("noSession")}</p>
      <Field id="opening-float" label={t("open.float")}>
        <Input
          id="opening-float"
          inputMode="decimal"
          className={`${FIELD} font-mono`}
          value={float}
          onChange={(e) => setFloat(e.target.value)}
        />
      </Field>
      <Alert message={localError ?? serverError} />
      <Button type="submit" size="pos-lg" disabled={openSession.isPending}>
        <Banknote aria-hidden />
        {t("open.submit")}
      </Button>
    </form>
  );
}

type Mode =
  { kind: "movement"; movement: DrawerMovementKind } | { kind: "noSale" } | { kind: "close" } | null;
type Pending = { perm: Perm; retry: (approvalId: string) => void } | null;

function MovementForm({
  movement,
  onDone,
  onApproval,
}: {
  movement: DrawerMovementKind;
  onDone: () => void;
  onApproval: (p: Pending) => void;
}) {
  const { t, i18n } = useTranslation(["cash", "common"]);
  const move = useDrawerMovement();
  const [amount, setAmount] = useState("");
  const [reason, setReason] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const serverError =
    move.error instanceof CommandError && !move.error.needsApproval
      ? translateError(i18n.t, move.error.error)
      : null;

  const run = (approvalId: string | null) => {
    const cents = parseMajor(amount);
    if (cents === null) {
      setLocalError(t("moneyInvalid"));
      playCue("error");
      return;
    }
    setLocalError(null);
    move.mutate(
      { input: { kind: movement, amount: cents, reason }, approvalId },
      {
        onSuccess: () => {
          playCue("complete");
          onDone();
        },
        onError: (e) => {
          if (e instanceof CommandError && e.needsApproval) {
            onApproval({ perm: "CASH_PAY_IN_OUT", retry: (id) => run(id) });
          } else {
            playCue("error");
          }
        },
      },
    );
  };

  const title = movement === "pay_in" ? t("movement.payInTitle") : t("movement.payOutTitle");
  return (
    <form
      aria-label={title}
      className="flex flex-col gap-3 rounded-md bg-muted p-3"
      onSubmit={(e) => {
        e.preventDefault();
        run(null);
      }}
    >
      <h3 className="font-semibold">{title}</h3>
      <Field id="movement-amount" label={t("movement.amount")}>
        <Input
          id="movement-amount"
          inputMode="decimal"
          className={`${FIELD} font-mono`}
          value={amount}
          onChange={(e) => setAmount(e.target.value)}
        />
      </Field>
      <Field id="movement-reason" label={t("movement.reason")} hint={t("movement.reasonHint")}>
        <Input
          id="movement-reason"
          className={FIELD}
          value={reason}
          maxLength={120}
          onChange={(e) => setReason(e.target.value)}
        />
      </Field>
      <Alert message={localError ?? serverError} />
      <div className="flex gap-2">
        <Button type="submit" size="pos" disabled={move.isPending}>
          {t("movement.submit")}
        </Button>
        <Button type="button" size="pos" variant="outline" onClick={onDone}>
          {t("common:actions.cancel")}
        </Button>
      </div>
    </form>
  );
}

function NoSaleForm({ onDone, onApproval }: { onDone: () => void; onApproval: (p: Pending) => void }) {
  const { t, i18n } = useTranslation(["cash", "common"]);
  const noSale = useNoSale();
  const [reason, setReason] = useState("");
  const serverError =
    noSale.error instanceof CommandError && !noSale.error.needsApproval
      ? translateError(i18n.t, noSale.error.error)
      : null;
  const run = (approvalId: string | null) =>
    noSale.mutate(
      { reason, approvalId },
      {
        onSuccess: () => {
          playCue("complete");
          onDone();
        },
        onError: (e) => {
          if (e instanceof CommandError && e.needsApproval) {
            onApproval({ perm: "DRAWER_NO_SALE", retry: (id) => run(id) });
          } else {
            playCue("error");
          }
        },
      },
    );
  return (
    <form
      aria-label={t("noSale.title")}
      className="flex flex-col gap-3 rounded-md bg-muted p-3"
      onSubmit={(e) => {
        e.preventDefault();
        run(null);
      }}
    >
      <h3 className="font-semibold">{t("noSale.title")}</h3>
      <Field id="no-sale-reason" label={t("noSale.reason")}>
        <Input
          id="no-sale-reason"
          className={FIELD}
          value={reason}
          maxLength={120}
          onChange={(e) => setReason(e.target.value)}
        />
      </Field>
      <Alert message={serverError} />
      <div className="flex gap-2">
        <Button type="submit" size="pos" disabled={noSale.isPending}>
          {t("noSale.submit")}
        </Button>
        <Button type="button" size="pos" variant="outline" onClick={onDone}>
          {t("common:actions.cancel")}
        </Button>
      </div>
    </form>
  );
}

function CloseForm({ onDone, onClosed }: { onDone: () => void; onClosed: (z: ZReport) => void }) {
  const { t, i18n } = useTranslation(["cash", "common"]);
  const close = useCloseCashSession();
  const [counted, setCounted] = useState("");
  const [byDenomination, setByDenomination] = useState(false);
  const [counts, setCounts] = useState<Record<number, string>>({});
  const [note, setNote] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const serverError = close.error instanceof CommandError ? translateError(i18n.t, close.error.error) : null;

  const total = byDenomination ? denominationTotal(counts) : parseMajor(counted);

  const submit = () => {
    if (total === null) {
      setLocalError(t("moneyInvalid"));
      playCue("error");
      return;
    }
    setLocalError(null);
    const denominations = byDenomination
      ? DENOMINATIONS.filter((v) => (counts[v] ?? "").trim() !== "").map((value) => ({
          value,
          count: Number.parseInt(counts[value] ?? "0", 10),
        }))
      : null;
    close.mutate(
      { counted_cash: total, denominations, note: note.trim() === "" ? null : note },
      {
        onSuccess: (z) => {
          playCue("complete");
          onClosed(z);
        },
        onError: () => {
          setConfirming(false);
          playCue("error");
        },
      },
    );
  };

  return (
    <form
      aria-label={t("close.title")}
      className="flex flex-col gap-3 rounded-md bg-muted p-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (confirming) submit();
        else setConfirming(true);
      }}
    >
      <h3 className="font-semibold">{t("close.title")}</h3>
      <label className="flex min-h-12 items-center gap-3">
        <input
          type="checkbox"
          className="size-6 accent-primary"
          checked={byDenomination}
          onChange={(e) => setByDenomination(e.target.checked)}
        />
        {t("close.byDenomination")}
      </label>
      {byDenomination ? (
        <div className="grid grid-cols-2 gap-2 xl:grid-cols-4">
          {DENOMINATIONS.map((v) => (
            <Field
              key={v}
              id={`denom-${v}`}
              label={t("close.denomination", { value: formatMoney(v).replace("Rs ", "") })}
            >
              <Input
                id={`denom-${v}`}
                inputMode="numeric"
                className={`${FIELD} font-mono`}
                value={counts[v] ?? ""}
                onChange={(e) => setCounts((c) => ({ ...c, [v]: e.target.value }))}
              />
            </Field>
          ))}
        </div>
      ) : (
        <Field id="counted-cash" label={t("close.counted")}>
          <Input
            id="counted-cash"
            inputMode="decimal"
            className={`${FIELD} font-mono`}
            value={counted}
            onChange={(e) => setCounted(e.target.value)}
          />
        </Field>
      )}
      {byDenomination && total !== null && (
        <p className="font-mono text-lg" aria-live="polite">
          {formatMoney(total)}
        </p>
      )}
      <Field id="close-note" label={t("close.note")}>
        <Input
          id="close-note"
          className={FIELD}
          value={note}
          maxLength={500}
          onChange={(e) => setNote(e.target.value)}
        />
      </Field>
      <Alert message={localError ?? serverError} />
      {confirming && <p role="status">{t("close.confirm")}</p>}
      <div className="flex gap-2">
        <Button type="submit" size="pos" disabled={close.isPending}>
          {confirming ? t("close.confirmYes") : t("close.submit")}
        </Button>
        <Button type="button" size="pos" variant="outline" onClick={onDone}>
          {t("common:actions.cancel")}
        </Button>
      </div>
    </form>
  );
}

function SessionPanel({ session, onClosed }: { session: CashSession; onClosed: (z: ZReport) => void }) {
  const { t, i18n } = useTranslation(["cash", "common"]);
  const [mode, setMode] = useState<Mode>(null);
  const [pending, setPending] = useState<Pending>(null);
  const done = () => setMode(null);
  const opened = new Date(session.opened_at).toLocaleTimeString(
    i18n.resolvedLanguage === "si" ? "si-LK" : "en-LK",
  );

  return (
    <section
      aria-labelledby="session-title"
      className="flex flex-col gap-4 rounded-lg border border-border p-4"
    >
      <header>
        <h2 id="session-title" className="text-xl font-semibold">
          {t("session.title")}
        </h2>
        <p className="text-muted-foreground">
          {t("session.openedBy", { name: session.opened_by_name, time: opened })}
        </p>
        <p>
          {t("session.float")}: <span className="font-mono">{formatMoney(session.opening_float)}</span> ·{" "}
          {t("session.businessDate")}: <span className="font-mono">{session.business_date}</span>
        </p>
      </header>
      <div className="flex flex-wrap gap-2">
        <Button
          type="button"
          size="pos"
          variant="outline"
          onClick={() => setMode({ kind: "movement", movement: "pay_in" })}
        >
          <Plus aria-hidden />
          {t("actions.payIn")}
        </Button>
        <Button
          type="button"
          size="pos"
          variant="outline"
          onClick={() => setMode({ kind: "movement", movement: "pay_out" })}
        >
          <Minus aria-hidden />
          {t("actions.payOut")}
        </Button>
        <Button type="button" size="pos" variant="outline" onClick={() => setMode({ kind: "noSale" })}>
          <DoorOpen aria-hidden />
          {t("actions.noSale")}
        </Button>
        <Can perm="CASH_SESSION_MANAGE">
          <Button type="button" size="pos" onClick={() => setMode({ kind: "close" })}>
            <LockKeyhole aria-hidden />
            {t("actions.close")}
          </Button>
        </Can>
      </div>
      {mode?.kind === "movement" && (
        <MovementForm key={mode.movement} movement={mode.movement} onDone={done} onApproval={setPending} />
      )}
      {mode?.kind === "noSale" && <NoSaleForm onDone={done} onApproval={setPending} />}
      {mode?.kind === "close" && <CloseForm onDone={done} onClosed={onClosed} />}
      {pending && (
        <ApprovalDialog
          open
          perm={pending.perm}
          targetRef={DRAWER_TARGET}
          onApproved={(id) => {
            const retry = pending.retry;
            setPending(null);
            retry(id);
          }}
          onCancel={() => setPending(null)}
        />
      )}
    </section>
  );
}

/** The counter's cash drawer: open a session, pay in/out, no-sale, close with the Z-report (doc §3.4). */
export function CashScreen() {
  const { t, i18n } = useTranslation(["cash", "common"]);
  const current = useCurrentCashSession();
  const [closed, setClosed] = useState<ZReport | null>(null);
  const loadError =
    current.error instanceof CommandError ? translateError(i18n.t, current.error.error) : null;

  return (
    <section className="flex flex-col gap-4">
      <h1 className="text-2xl font-semibold">{t("title")}</h1>
      <Alert message={loadError} />
      {current.isLoading ? (
        <p className="text-muted-foreground">{t("common:table.loading")}</p>
      ) : current.data ? (
        <SessionPanel session={current.data} onClosed={setClosed} />
      ) : (
        <Can perm="CASH_SESSION_MANAGE" fallback={<p>{t("noSession")}</p>}>
          <OpenSessionForm />
        </Can>
      )}
      {closed && <ZReportView report={closed} />}
    </section>
  );
}
