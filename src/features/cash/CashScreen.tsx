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
  useCashSessionReport,
  useCloseCashSession,
  useCurrentCashSession,
  useDrawerMovement,
  useNoSale,
  useOpenCashSession,
} from "@/db/hooks";
import { translateError } from "@/i18n";
import { playCue } from "@/lib/audio";
import { formatMoney, parseMajor } from "@/lib/money";
import { MovementList, ZReportView } from "./ZReportView";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { CashHistory } from "./CashHistory";
import { PrintZButton } from "./PrintZButton";

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

function CloseForm({
  expected,
  onDone,
  onClosed,
}: {
  expected: number | null;
  onDone: () => void;
  onClosed: (z: ZReport) => void;
}) {
  const { t, i18n } = useTranslation(["cash", "common"]);
  const close = useCloseCashSession();
  const [counted, setCounted] = useState("");
  const [byTotal, setByTotal] = useState(false);
  const [counts, setCounts] = useState<Record<number, string>>({});
  const [note, setNote] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const serverError = close.error instanceof CommandError ? translateError(i18n.t, close.error.error) : null;

  const total = byTotal ? parseMajor(counted) : denominationTotal(counts);
  // Over / short, said in words (never colour alone) before anything is confirmed.
  let verdict: string | null = null;
  if (expected !== null && total !== null) {
    const diff = total - expected;
    verdict =
      diff === 0
        ? t("close.diff.exact")
        : diff < 0
          ? t("close.diff.short", { amount: formatMoney(-diff) })
          : t("close.diff.over", { amount: formatMoney(diff) });
  }

  const submit = () => {
    if (total === null) {
      setLocalError(t("moneyInvalid"));
      playCue("error");
      return;
    }
    setLocalError(null);
    const denominations = byTotal
      ? null
      : DENOMINATIONS.filter((v) => (counts[v] ?? "").trim() !== "").map((value) => ({
          value,
          count: Number.parseInt(counts[value] ?? "0", 10),
        }));
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
      className="flex flex-col gap-4 rounded-lg border border-border bg-muted/40 p-4"
      onSubmit={(e) => {
        e.preventDefault();
        if (confirming) submit();
        else if (total === null) {
          setLocalError(t("moneyInvalid"));
          playCue("error");
        } else {
          setLocalError(null);
          setConfirming(true);
        }
      }}
    >
      <h3 className="text-lg font-semibold">{t("close.title")}</h3>
      {byTotal ? (
        <Field id="counted-cash" label={t("close.counted")}>
          <Input
            id="counted-cash"
            inputMode="decimal"
            className={`${FIELD} font-mono`}
            value={counted}
            onChange={(e) => setCounted(e.target.value)}
          />
        </Field>
      ) : (
        <>
          <p className="text-muted-foreground">{t("close.countHint")}</p>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 xl:grid-cols-4">
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
        </>
      )}
      <button
        type="button"
        className="self-start text-base underline underline-offset-4"
        onClick={() => setByTotal((v) => !v)}
      >
        {byTotal ? t("close.enterNotes") : t("close.enterTotal")}
      </button>
      <dl className="grid grid-cols-2 gap-y-1 rounded-md border bg-background p-3 text-lg" aria-live="polite">
        {expected !== null && (
          <>
            <dt className="text-muted-foreground">{t("close.shouldBe")}</dt>
            <dd className="text-right font-mono">{formatMoney(expected)}</dd>
          </>
        )}
        <dt className="text-muted-foreground">{t("close.youCounted")}</dt>
        <dd className="text-right font-mono">{total === null ? "–" : formatMoney(total)}</dd>
        {verdict && <dd className="col-span-2 pt-1 text-xl font-semibold">{verdict}</dd>}
      </dl>
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
        <Button
          type="button"
          size="pos"
          variant="outline"
          onClick={confirming ? () => setConfirming(false) : onDone}
        >
          {confirming ? t("close.back") : t("common:actions.cancel")}
        </Button>
      </div>
    </form>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col">
      <dt className="text-sm text-muted-foreground">{label}</dt>
      <dd className="font-mono text-lg font-semibold">{value}</dd>
    </div>
  );
}

/** The open shift: what should be in the drawer right now, the four things a cashier does, and the day's cash movements. */
function SessionPanel({ session, onClosed }: { session: CashSession; onClosed: (z: ZReport) => void }) {
  const { t, i18n } = useTranslation(["cash", "common"]);
  const live = useCashSessionReport(session.id).data ?? null;
  const [mode, setMode] = useState<Mode>(null);
  const [pending, setPending] = useState<Pending>(null);
  const done = () => setMode(null);
  const opened = new Date(session.opened_at).toLocaleTimeString(
    i18n.resolvedLanguage === "si" ? "si-LK" : "en-LK",
    { hour: "2-digit", minute: "2-digit" },
  );

  return (
    <section aria-labelledby="session-title" className="flex flex-col gap-5">
      <div className="flex flex-col gap-4 rounded-lg border border-border p-5">
        <header>
          <h2 id="session-title" className="text-xl font-semibold">
            {t("session.title")}
          </h2>
          <p className="text-muted-foreground">
            {t("session.openedBy", { name: session.opened_by_name, time: opened })} · {session.business_date}
          </p>
        </header>
        <div>
          <p className="text-muted-foreground">{t("live.expected")}</p>
          <p className="text-5xl leading-tight font-bold tabular-nums" data-testid="expected-cash">
            {formatMoney(live?.expected_cash ?? session.opening_float)}
          </p>
        </div>
        <dl className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-6">
          <Stat label={t("z.float")} value={formatMoney(live?.opening_float ?? session.opening_float)} />
          <Stat label={t("z.cashSales")} value={formatMoney(live?.cash_sales ?? 0)} />
          <Stat label={t("z.payIns")} value={formatMoney(live?.pay_ins ?? 0)} />
          <Stat label={t("z.payOuts")} value={formatMoney(live?.pay_outs ?? 0)} />
          <Stat label={t("z.sales")} value={String(live?.sales_count ?? 0)} />
          <Stat label={t("live.sales")} value={formatMoney(live?.sales_total ?? 0)} />
        </dl>
      </div>

      <div className="flex flex-wrap gap-3">
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
        <span className="flex-1" />
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
      {mode?.kind === "close" && (
        <CloseForm expected={live?.expected_cash ?? null} onDone={done} onClosed={onClosed} />
      )}

      <section aria-labelledby="movements-title" className="flex flex-col gap-2">
        <h3 id="movements-title" className="text-lg font-semibold">
          {t("movements.title")}
        </h3>
        {live && live.drawer.length > 0 ? (
          <MovementList items={live.drawer} />
        ) : (
          <p className="text-muted-foreground">{t("movements.empty")}</p>
        )}
      </section>

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

function Today() {
  const { t, i18n } = useTranslation(["cash", "common"]);
  const current = useCurrentCashSession();
  const [closed, setClosed] = useState<ZReport | null>(null);
  const loadError =
    current.error instanceof CommandError ? translateError(i18n.t, current.error.error) : null;

  if (closed) {
    return (
      <ZReportView
        report={closed}
        actions={
          <>
            <PrintZButton sessionId={closed.session.id} />
            <Button type="button" size="pos" onClick={() => setClosed(null)}>
              {t("z.done")}
            </Button>
          </>
        }
      />
    );
  }
  return (
    <>
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
    </>
  );
}

/** The counter's cash drawer (doc §3.4): Today (open, pay in/out, no-sale, guided close) and History. */
export function CashScreen() {
  const { t } = useTranslation("cash");
  return (
    <section className="flex flex-col gap-4">
      <h1 className="text-2xl font-semibold">{t("title")}</h1>
      <Tabs defaultValue="today" className="gap-4">
        <TabsList aria-label={t("tabs.label")}>
          <TabsTrigger value="today" className="h-12 px-6 text-base">
            {t("tabs.today")}
          </TabsTrigger>
          <TabsTrigger value="history" className="h-12 px-6 text-base">
            {t("tabs.history")}
          </TabsTrigger>
        </TabsList>
        <TabsContent value="today" className="flex flex-col gap-4">
          <Today />
        </TabsContent>
        <TabsContent value="history">
          <CashHistory />
        </TabsContent>
      </Tabs>
    </section>
  );
}
