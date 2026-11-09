// Expenses (doc §5.1, EXPENSE_MANAGE): the list for any §5 period with totals by category, entry and edit
// (optionally paid from this counter's drawer: a pay-out in the open session), delete, and the categories.
// Batch purchases are never expenses: they go through Batches (inventory → COGS when sold).
import type { ColumnDef } from "@tanstack/react-table";
import { Plus, Tags, Trash2 } from "lucide-react";
import { useMemo, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ApprovalDialog } from "@/components/auth/ApprovalDialog";
import { DataTable } from "@/components/data-table/DataTable";
import { PeriodPicker, periodFor } from "@/components/period/PeriodPicker";
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
import type { ExpenseCategory, ExpenseRow, PaymentMethod, Period } from "@/db/bindings";
import { CommandError } from "@/db/client";
import {
  useBusinessToday,
  useCurrentCashSession,
  useDeleteExpense,
  useExpenseCategories,
  useExpenseList,
  useRecordExpense,
  useSaveExpenseCategory,
  useUpdateExpense,
} from "@/db/hooks";
import { translateError } from "@/i18n";
import { formatMoney, parseMajor, toMajorInput } from "@/lib/money";

/** Must match `commands::cash::DRAWER_TARGET`. */
export const DRAWER_TARGET = "cash_drawer";
const FIELD = "h-12 text-base";
const SELECT = "h-12 w-full rounded-md border border-input bg-background px-3 text-base";
const METHODS: PaymentMethod[] = ["cash", "card", "bank_transfer", "qr", "other"];

function ErrorText({ error }: { error: unknown }) {
  const { i18n } = useTranslation();
  if (!(error instanceof CommandError) || error.error.kind === "APPROVAL_REQUIRED") return null;
  return (
    <p role="alert" className="text-base text-destructive">
      {translateError(i18n.t, error.error)}
    </p>
  );
}

/** Run a drawer write; on ApprovalRequired, ask a supervisor (CASH_PAY_IN_OUT) and retry. */
function useApproved() {
  const [retry, setRetry] = useState<((id: string) => void) | null>(null);
  const run = (send: (approvalId: string | null) => Promise<unknown>) => {
    send(null).catch((e: unknown) => {
      if (e instanceof CommandError && e.error.kind === "APPROVAL_REQUIRED")
        setRetry(() => (id: string) => void send(id).catch(() => undefined));
    });
  };
  const dialog = retry ? (
    <ApprovalDialog
      open
      perm="CASH_PAY_IN_OUT"
      targetRef={DRAWER_TARGET}
      onApproved={(id) => {
        const r = retry;
        setRetry(null);
        r(id);
      }}
      onCancel={() => setRetry(null)}
    />
  ) : null;
  return { run, dialog };
}

function ExpenseDialog({
  categories,
  today,
  editing,
  onClose,
}: {
  categories: ExpenseCategory[];
  today: string;
  editing: ExpenseRow | null;
  onClose: () => void;
}) {
  const { t } = useTranslation(["expenses", "common"]);
  const session = useCurrentCashSession();
  const record = useRecordExpense();
  const update = useUpdateExpense();
  const { run, dialog } = useApproved();
  const active = categories.filter((c) => c.is_active || c.id === editing?.category_id);
  const [categoryId, setCategoryId] = useState(editing?.category_id ?? active[0]?.id ?? "");
  const [amount, setAmount] = useState(editing ? toMajorInput(editing.amount) : "");
  const [date, setDate] = useState(editing?.business_date ?? today);
  const [method, setMethod] = useState<PaymentMethod>(
    (editing?.payment_method as PaymentMethod | undefined) ?? "cash",
  );
  const [drawer, setDrawer] = useState(editing?.paid_from_drawer ?? false);
  const [payee, setPayee] = useState(editing?.payee ?? "");
  const [note, setNote] = useState(editing?.note ?? "");
  const locked = editing?.paid_from_drawer ?? false;
  const amountN = parseMajor(amount);
  const sessionOpen = session.data !== null && session.data !== undefined;
  const valid = categoryId !== "" && amountN !== null && amountN > 0 && /^\d{4}-\d{2}-\d{2}$/.test(date);
  const pending = record.isPending || update.isPending;
  const error = editing ? update.error : record.error;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!valid || amountN === null) return;
    const input = {
      category_id: categoryId,
      amount: amountN,
      business_date: date,
      payment_method: drawer ? ("cash" as const) : method,
      paid_from_drawer: drawer,
      payee,
      note,
    };
    const done = () => {
      toast.success(t(editing ? "saved" : "recorded"));
      onClose();
    };
    if (editing) update.mutate({ id: editing.id, input }, { onSuccess: done });
    else
      run((approvalId) =>
        record.mutateAsync({ input, approvalId }).then((r) => {
          done();
          return r;
        }),
      );
  };

  return (
    <Dialog open onOpenChange={(o) => !o && !pending && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t(editing ? "edit" : "add")}</DialogTitle>
          <DialogDescription>{t("notBatches")}</DialogDescription>
        </DialogHeader>
        <form
          onSubmit={submit}
          className="grid gap-3 sm:grid-cols-2"
          aria-label={t(editing ? "edit" : "add")}
        >
          <div className="flex flex-col gap-2 sm:col-span-2">
            <Label htmlFor="exp-category">{t("fields.category")}</Label>
            <select
              id="exp-category"
              className={SELECT}
              value={categoryId}
              onChange={(e) => setCategoryId(e.target.value)}
            >
              {active.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="exp-amount">{t("fields.amount")}</Label>
            <Input
              id="exp-amount"
              className={FIELD}
              inputMode="decimal"
              disabled={locked}
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="exp-date">{t("fields.date")}</Label>
            <Input
              id="exp-date"
              type="date"
              className={FIELD}
              max={today}
              disabled={locked || drawer}
              value={drawer ? today : date}
              onChange={(e) => setDate(e.target.value)}
            />
          </div>
          {!editing && (
            <label className="flex min-h-12 items-center gap-3 text-base sm:col-span-2">
              <input
                type="checkbox"
                className="size-6"
                checked={drawer}
                disabled={!sessionOpen}
                onChange={(e) => {
                  setDrawer(e.target.checked);
                  if (e.target.checked) setDate(today);
                }}
              />
              {sessionOpen ? t("fields.fromDrawer") : t("fields.noSession")}
            </label>
          )}
          {locked && <p className="text-sm text-muted-foreground sm:col-span-2">{t("lockedHint")}</p>}
          <div className="flex flex-col gap-2">
            <Label htmlFor="exp-method">{t("fields.method")}</Label>
            <select
              id="exp-method"
              className={SELECT}
              disabled={drawer || locked}
              value={drawer ? "cash" : method}
              onChange={(e) => setMethod(e.target.value as PaymentMethod)}
            >
              {METHODS.map((m) => (
                <option key={m} value={m}>
                  {t(`methods.${m}`)}
                </option>
              ))}
            </select>
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="exp-payee">{t("fields.payee")}</Label>
            <Input
              id="exp-payee"
              className={FIELD}
              maxLength={200}
              value={payee}
              onChange={(e) => setPayee(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-2 sm:col-span-2">
            <Label htmlFor="exp-note">{t("fields.note")}</Label>
            <Input
              id="exp-note"
              className={FIELD}
              maxLength={200}
              value={note}
              onChange={(e) => setNote(e.target.value)}
            />
          </div>
          <div className="sm:col-span-2">
            <ErrorText error={error} />
          </div>
          <DialogFooter className="gap-2 sm:col-span-2">
            <Button type="button" size="pos" variant="outline" disabled={pending} onClick={onClose}>
              {t("common:actions.cancel")}
            </Button>
            <Button type="submit" size="pos" disabled={!valid || pending}>
              {t("common:actions.save")}
            </Button>
          </DialogFooter>
        </form>
        {dialog}
      </DialogContent>
    </Dialog>
  );
}

function DeleteDialog({ expense, onClose }: { expense: ExpenseRow; onClose: () => void }) {
  const { t } = useTranslation(["expenses", "common"]);
  const remove = useDeleteExpense();
  const { run, dialog } = useApproved();
  return (
    <Dialog open onOpenChange={(o) => !o && !remove.isPending && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("delete.title")}</DialogTitle>
          <DialogDescription>
            {expense.paid_from_drawer ? t("delete.drawer") : t("delete.plain")}
          </DialogDescription>
        </DialogHeader>
        <p className="text-base">
          {expense.category_name} · {formatMoney(expense.amount)} · {expense.business_date}
        </p>
        <ErrorText error={remove.error} />
        <DialogFooter className="gap-2">
          <Button type="button" size="pos" variant="outline" disabled={remove.isPending} onClick={onClose}>
            {t("common:actions.cancel")}
          </Button>
          <Button
            type="button"
            size="pos"
            variant="destructive"
            disabled={remove.isPending}
            onClick={() =>
              run((approvalId) =>
                remove.mutateAsync({ id: expense.id, approvalId }).then(() => {
                  toast.success(t("delete.done"));
                  onClose();
                }),
              )
            }
          >
            <Trash2 aria-hidden />
            {t("delete.confirm")}
          </Button>
        </DialogFooter>
        {dialog}
      </DialogContent>
    </Dialog>
  );
}

function CategoryRow({ category }: { category: ExpenseCategory }) {
  const { t } = useTranslation(["expenses", "common"]);
  const save = useSaveExpenseCategory();
  const [name, setName] = useState(category.name);
  return (
    <li className="flex flex-wrap items-center gap-2">
      <Input
        aria-label={t("categories.name")}
        className={`${FIELD} min-w-48 flex-1`}
        maxLength={60}
        value={name}
        onChange={(e) => setName(e.target.value)}
      />
      <Button
        type="button"
        size="pos"
        variant="outline"
        disabled={save.isPending || name.trim() === "" || name === category.name}
        onClick={() => save.mutate({ id: category.id, input: { name, is_active: category.is_active } })}
      >
        {t("common:actions.save")}
      </Button>
      <Button
        type="button"
        size="pos"
        variant="outline"
        disabled={save.isPending}
        onClick={() =>
          save.mutate({ id: category.id, input: { name: category.name, is_active: !category.is_active } })
        }
      >
        {category.is_active ? t("categories.retire") : t("categories.restore")}
      </Button>
      <span className="w-24 text-sm text-muted-foreground">
        {t("categories.used", { count: category.used })}
      </span>
      <div className="basis-full">
        <ErrorText error={save.error} />
      </div>
    </li>
  );
}

function Categories({ categories }: { categories: ExpenseCategory[] }) {
  const { t } = useTranslation(["expenses", "common"]);
  const save = useSaveExpenseCategory();
  const [name, setName] = useState("");
  const add = (e: FormEvent) => {
    e.preventDefault();
    save.mutate({ id: null, input: { name, is_active: true } }, { onSuccess: () => setName("") });
  };
  return (
    <section className="flex flex-col gap-3 rounded-lg border p-4" aria-label={t("categories.title")}>
      <h2 className="text-xl font-semibold">{t("categories.title")}</h2>
      <ul className="flex flex-col gap-2">
        {categories.map((c) => (
          <CategoryRow key={`${c.id}|${c.name}|${c.is_active}`} category={c} />
        ))}
      </ul>
      <form onSubmit={add} className="flex flex-wrap items-end gap-2">
        <div className="flex min-w-48 flex-1 flex-col gap-2">
          <Label htmlFor="exp-new-category">{t("categories.add")}</Label>
          <Input
            id="exp-new-category"
            className={FIELD}
            maxLength={60}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </div>
        <Button type="submit" size="pos" disabled={save.isPending || name.trim() === ""}>
          <Plus aria-hidden />
          {t("categories.addButton")}
        </Button>
      </form>
      <ErrorText error={save.error} />
    </section>
  );
}

function Expenses({ today }: { today: string }) {
  const { t } = useTranslation(["expenses", "common"]);
  const [period, setPeriod] = useState<Period>(() => periodFor("month", today));
  const [categoryId, setCategoryId] = useState("");
  const categories = useExpenseCategories();
  const list = useExpenseList(period, categoryId || null);
  const [editing, setEditing] = useState<ExpenseRow | "new" | null>(null);
  const [deleting, setDeleting] = useState<ExpenseRow | null>(null);
  const [showCategories, setShowCategories] = useState(false);
  const data = list.data;
  const columns = useMemo<ColumnDef<ExpenseRow>[]>(
    () => [
      { id: "date", header: t("fields.date"), accessorFn: (r) => r.business_date, enableSorting: false },
      {
        id: "category",
        header: t("fields.category"),
        accessorFn: (r) => r.category_name,
        enableSorting: false,
      },
      {
        id: "amount",
        header: t("fields.amount"),
        accessorFn: (r) => formatMoney(r.amount),
        enableSorting: false,
      },
      {
        id: "method",
        header: t("fields.method"),
        accessorFn: (r) => (r.paid_from_drawer ? t("drawer") : t(`methods.${r.payment_method}`)),
        enableSorting: false,
      },
      { id: "payee", header: t("fields.payee"), accessorFn: (r) => r.payee ?? "", enableSorting: false },
      {
        id: "actions",
        header: "",
        enableSorting: false,
        cell: ({ row }) => (
          <Button
            type="button"
            size="pos"
            variant="ghost"
            aria-label={t("delete.title")}
            onClick={(e) => {
              e.stopPropagation();
              setDeleting(row.original);
            }}
          >
            <Trash2 aria-hidden />
          </Button>
        ),
      },
    ],
    [t],
  );
  return (
    <section className="flex flex-col gap-4">
      <header className="flex flex-wrap items-center gap-3">
        <h1 className="flex-1 text-2xl font-semibold">{t("title")}</h1>
        <Button type="button" size="pos" variant="outline" onClick={() => setShowCategories((s) => !s)}>
          <Tags aria-hidden />
          {t("categories.title")}
        </Button>
        <Button
          type="button"
          size="pos"
          disabled={!categories.data || categories.data.every((c) => !c.is_active)}
          onClick={() => setEditing("new")}
        >
          <Plus aria-hidden />
          {t("add")}
        </Button>
      </header>
      {showCategories && categories.data && <Categories categories={categories.data} />}
      <PeriodPicker value={period} onChange={setPeriod} today={today} />
      <div className="flex max-w-sm flex-col gap-2">
        <Label htmlFor="exp-filter">{t("fields.category")}</Label>
        <select
          id="exp-filter"
          className={SELECT}
          value={categoryId}
          onChange={(e) => setCategoryId(e.target.value)}
        >
          <option value="">{t("allCategories")}</option>
          {(categories.data ?? []).map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
      </div>
      <ErrorText error={list.error ?? categories.error} />
      {data && (
        <div className="grid gap-4 xl:grid-cols-[1fr_22rem]">
          <DataTable<ExpenseRow>
            ariaLabel={t("title")}
            columns={columns}
            data={data.rows}
            getRowId={(r) => r.id}
            height={432}
            emptyLabel={t("empty")}
            onRowActivate={(r) => setEditing(r)}
          />
          <section className="flex flex-col gap-2 rounded-lg border p-4" aria-label={t("totals.title")}>
            <h2 className="text-lg font-semibold">{t("totals.title")}</h2>
            <ul className="text-base">
              {data.by_category.map((c) => (
                <li key={c.category_id} className="flex gap-2">
                  <span className="flex-1">{c.name}</span>
                  <span className="font-mono">{formatMoney(c.amount)}</span>
                </li>
              ))}
            </ul>
            <p className="flex border-t pt-2 text-lg font-semibold">
              <span className="flex-1">{t("totals.total")}</span>
              <span className="font-mono">{formatMoney(data.total)}</span>
            </p>
            <p className="flex text-sm text-muted-foreground">
              <span className="flex-1">{t("totals.fromDrawer")}</span>
              <span className="font-mono">{formatMoney(data.from_drawer)}</span>
            </p>
          </section>
        </div>
      )}
      {editing && categories.data && (
        <ExpenseDialog
          key={editing === "new" ? "new" : editing.id}
          categories={categories.data}
          today={today}
          editing={editing === "new" ? null : editing}
          onClose={() => setEditing(null)}
        />
      )}
      {deleting && <DeleteDialog expense={deleting} onClose={() => setDeleting(null)} />}
    </section>
  );
}

export function ExpensesScreen() {
  const today = useBusinessToday();
  return today.data ? <Expenses today={today.data} /> : null;
}
