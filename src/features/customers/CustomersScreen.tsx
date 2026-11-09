// Customer back office (doc §3.8, CUSTOMER_VIEW): the list (search by phone or name; sort by spend, visits,
// last visit or name), the profile (totals, average basket, top products and categories, name and note),
// purchase history with receipt drill-down, PDPA erasure, and analytics for any §5 period.
import type { ColumnDef } from "@tanstack/react-table";
import { BarChart3, Eraser, Save, Users } from "lucide-react";
import { useMemo, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { TopCustomersBar } from "@/components/charts/TopCustomersBar";
import { DataTable } from "@/components/data-table/DataTable";
import { ExportButton } from "@/components/export/ExportButton";
import { PeriodPicker, periodFor } from "@/components/period/PeriodPicker";
import { ReceiptDialog } from "@/components/receipt/ReceiptDialog";
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
import type {
  CustomerProfile,
  CustomerRow,
  CustomerSort,
  Granularity,
  HistoryOrder,
  Period,
  TopItem,
} from "@/db/bindings";
import { CommandError } from "@/db/client";
import {
  useBusinessToday,
  useCustomerAnalytics,
  useCustomerHistory,
  useCustomerList,
  useCustomerProfile,
  useCustomerReceipt,
  useEraseCustomer,
  useUpdateCustomer,
} from "@/db/hooks";
import { translateError } from "@/i18n";
import { formatMoney } from "@/lib/money";

const FIELD = "h-12 text-base";
const SORTS: CustomerSort[] = ["spend", "visits", "last_visit", "name"];
type Tab = "list" | "analytics";

/** `7500` bp → `75.0%`. */
export function formatBp(bp: number): string {
  return `${Math.trunc(bp / 100)}.${Math.trunc((bp % 100) / 10)}%`;
}

function ErrorText({ error }: { error: unknown }) {
  const { i18n } = useTranslation();
  if (!(error instanceof CommandError)) return null;
  return (
    <p role="alert" className="text-base text-destructive">
      {translateError(i18n.t, error.error)}
    </p>
  );
}

function Receipt({
  customerId,
  orderId,
  onClose,
}: {
  customerId: string;
  orderId: string;
  onClose: () => void;
}) {
  const receipt = useCustomerReceipt(customerId, orderId);
  return <ReceiptDialog receipt={receipt.data} error={receipt.error} onClose={onClose} />;
}

function History({ customerId }: { customerId: string }) {
  const { t } = useTranslation(["customers", "common"]);
  const pages = useCustomerHistory(customerId);
  const rows = useMemo(() => pages.data?.pages.flatMap((p) => p.rows) ?? [], [pages.data]);
  const [open, setOpen] = useState<string | null>(null);
  const columns = useMemo<ColumnDef<HistoryOrder>[]>(
    () => [
      { id: "date", header: t("history.date"), accessorFn: (r) => r.business_date, enableSorting: false },
      { id: "receipt", header: t("history.receipt"), accessorFn: (r) => r.receipt_no, enableSorting: false },
      { id: "items", header: t("history.items"), accessorFn: (r) => r.item_count, enableSorting: false },
      {
        id: "total",
        header: t("history.total"),
        enableSorting: false,
        cell: ({ row }) => (
          <span className="font-mono">
            {formatMoney(row.original.grand_total)}
            {row.original.status === "voided" && ` · ${t("history.voided")}`}
          </span>
        ),
      },
    ],
    [t],
  );
  return (
    <section className="flex flex-col gap-2" aria-label={t("history.title")}>
      <h3 className="text-lg font-semibold">{t("history.title")}</h3>
      <DataTable<HistoryOrder>
        ariaLabel={t("history.title")}
        columns={columns}
        data={rows}
        getRowId={(r) => r.id}
        height={288}
        rowHeight={48}
        emptyLabel={pages.isLoading ? t("common:table.loading") : t("history.empty")}
        onRowActivate={(r) => setOpen(r.id)}
        onEndReached={() => {
          if (pages.hasNextPage && !pages.isFetchingNextPage) void pages.fetchNextPage();
        }}
      />
      {open && <Receipt customerId={customerId} orderId={open} onClose={() => setOpen(null)} />}
    </section>
  );
}

function TopList({ title, items, none }: { title: string; items: TopItem[]; none: string }) {
  const { t } = useTranslation("customers");
  return (
    <div className="flex flex-col gap-1">
      <h3 className="text-base font-semibold">{title}</h3>
      {items.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("profile.nothingYet")}</p>
      ) : (
        <ul className="text-sm">
          {items.map((i) => (
            <li key={i.id ?? "none"} className="flex gap-2">
              <span className="flex-1">{i.name ?? none}</span>
              <span className="font-mono">×{i.qty}</span>
              <span className="w-28 text-right font-mono">{formatMoney(i.net)}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function EditForm({ profile }: { profile: CustomerProfile }) {
  const { t } = useTranslation(["customers", "common"]);
  const update = useUpdateCustomer();
  const [name, setName] = useState(profile.customer.display_name ?? "");
  const [note, setNote] = useState(profile.note ?? "");
  const submit = (e: FormEvent) => {
    e.preventDefault();
    update.mutate(
      { id: profile.customer.id, input: { display_name: name, note } },
      { onSuccess: () => toast.success(t("profile.saved")) },
    );
  };
  return (
    <form onSubmit={submit} className="flex flex-col gap-3" aria-label={t("profile.edit")}>
      <div className="flex flex-col gap-2">
        <Label htmlFor="cust-name">{t("profile.name")}</Label>
        <Input
          id="cust-name"
          className={FIELD}
          maxLength={60}
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor="cust-note">{t("profile.note")}</Label>
        <textarea
          id="cust-note"
          className="min-h-20 rounded-md border border-input bg-background p-3 text-base"
          maxLength={500}
          value={note}
          onChange={(e) => setNote(e.target.value)}
        />
      </div>
      <ErrorText error={update.error} />
      <Button type="submit" size="pos" disabled={update.isPending} className="self-start">
        <Save aria-hidden />
        {t("common:actions.save")}
      </Button>
    </form>
  );
}

function EraseButton({ id, onErased }: { id: string; onErased: () => void }) {
  const { t } = useTranslation(["customers", "common"]);
  const erase = useEraseCustomer();
  const [confirm, setConfirm] = useState(false);
  return (
    <>
      <Button type="button" size="pos" variant="destructive" onClick={() => setConfirm(true)}>
        <Eraser aria-hidden />
        {t("erase.action")}
      </Button>
      {confirm && (
        <Dialog open onOpenChange={(o) => !o && !erase.isPending && setConfirm(false)}>
          <DialogContent className="sm:max-w-lg">
            <DialogHeader>
              <DialogTitle>{t("erase.title")}</DialogTitle>
              <DialogDescription>{t("erase.explain")}</DialogDescription>
            </DialogHeader>
            <ErrorText error={erase.error} />
            <DialogFooter className="gap-2">
              <Button
                type="button"
                size="pos"
                variant="outline"
                disabled={erase.isPending}
                onClick={() => setConfirm(false)}
              >
                {t("common:actions.cancel")}
              </Button>
              <Button
                type="button"
                size="pos"
                variant="destructive"
                disabled={erase.isPending}
                onClick={() =>
                  erase.mutate(id, {
                    onSuccess: () => {
                      toast.success(t("erase.done"));
                      setConfirm(false);
                      onErased();
                    },
                  })
                }
              >
                {t("erase.confirm")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}
    </>
  );
}

function Profile({ id, onClose }: { id: string; onClose: () => void }) {
  const { t } = useTranslation(["customers", "common"]);
  const profile = useCustomerProfile(id);
  const p = profile.data;
  return (
    <section className="flex flex-col gap-4 rounded-lg border p-4" aria-label={t("profile.title")}>
      <ErrorText error={profile.error} />
      {p && (
        <>
          <header className="flex flex-wrap items-start gap-2">
            <div className="flex-1">
              <h2 className="text-xl font-semibold">{p.customer.display_name ?? t("unnamed")}</h2>
              <p className="font-mono text-base">{p.customer.phone_display ?? t("erased")}</p>
            </div>
            <Button type="button" size="pos" variant="outline" onClick={onClose}>
              {t("common:actions.close")}
            </Button>
          </header>
          <dl className="grid grid-cols-2 gap-x-6 gap-y-1 text-sm">
            <dt className="text-muted-foreground">{t("columns.visits")}</dt>
            <dd className="font-mono">{p.customer.visit_count}</dd>
            <dt className="text-muted-foreground">{t("columns.spend")}</dt>
            <dd className="font-mono">{formatMoney(p.customer.total_spend)}</dd>
            <dt className="text-muted-foreground">{t("columns.avg")}</dt>
            <dd className="font-mono">{formatMoney(p.customer.avg_basket)}</dd>
            <dt className="text-muted-foreground">{t("profile.firstVisit")}</dt>
            <dd>{p.customer.first_visit_date ?? "—"}</dd>
            <dt className="text-muted-foreground">{t("columns.lastVisit")}</dt>
            <dd>{p.customer.last_visit_date ?? "—"}</dd>
          </dl>
          <div className="grid gap-4 sm:grid-cols-2">
            <TopList title={t("profile.topProducts")} items={p.top_products} none={t("profile.quickSales")} />
            <TopList
              title={t("profile.topCategories")}
              items={p.top_categories}
              none={t("profile.uncategorised")}
            />
          </div>
          {p.erased_at === null ? (
            <>
              <EditForm key={`${p.customer.display_name}|${p.note}`} profile={p} />
              <div>
                <EraseButton id={p.customer.id} onErased={onClose} />
              </div>
            </>
          ) : (
            <p className="text-base">{t("erasedNote")}</p>
          )}
          <History customerId={p.customer.id} />
        </>
      )}
    </section>
  );
}

function CustomerList({ onOpen }: { onOpen: (id: string) => void }) {
  const { t } = useTranslation(["customers", "common"]);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<CustomerSort>("spend");
  const list = useCustomerList(search, sort);
  const rows = list.data?.rows ?? [];
  const columns = useMemo<ColumnDef<CustomerRow>[]>(
    () => [
      {
        id: "name",
        header: t("columns.name"),
        accessorFn: (r) => r.display_name ?? t("unnamed"),
        enableSorting: false,
      },
      {
        id: "phone",
        header: t("columns.phone"),
        accessorFn: (r) => r.phone_display ?? "",
        enableSorting: false,
      },
      { id: "visits", header: t("columns.visits"), accessorFn: (r) => r.visit_count, enableSorting: false },
      {
        id: "spend",
        header: t("columns.spend"),
        accessorFn: (r) => formatMoney(r.total_spend),
        enableSorting: false,
      },
      {
        id: "avg",
        header: t("columns.avg"),
        accessorFn: (r) => formatMoney(r.avg_basket),
        enableSorting: false,
      },
      {
        id: "last",
        header: t("columns.lastVisit"),
        accessorFn: (r) => r.last_visit_date ?? "—",
        enableSorting: false,
      },
    ],
    [t],
  );
  return (
    <section className="flex flex-col gap-3">
      <div className="flex flex-wrap items-end gap-3">
        <div className="flex min-w-64 flex-1 flex-col gap-2">
          <Label htmlFor="cust-search">{t("search")}</Label>
          <Input
            id="cust-search"
            type="search"
            className={FIELD}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <div className="flex flex-wrap gap-2" role="group" aria-label={t("sortBy")}>
          {SORTS.map((s) => (
            <Button
              key={s}
              type="button"
              size="pos"
              variant={sort === s ? "default" : "outline"}
              aria-pressed={sort === s}
              onClick={() => setSort(s)}
            >
              {t(`sorts.${s}`)}
            </Button>
          ))}
        </div>
      </div>
      <ErrorText error={list.error} />
      {list.data?.truncated && <p className="text-sm text-muted-foreground">{t("truncated")}</p>}
      <DataTable<CustomerRow>
        ariaLabel={t("title")}
        columns={columns}
        data={rows}
        getRowId={(r) => r.id}
        height={480}
        emptyLabel={list.isLoading ? t("common:table.loading") : t("empty")}
        onRowActivate={(r) => onOpen(r.id)}
      />
    </section>
  );
}

function Analytics({ today }: { today: string }) {
  const { t } = useTranslation(["customers", "common"]);
  const [period, setPeriod] = useState<Period>(() => periodFor("month", today));
  const [granularity, setGranularity] = useState<Granularity>("week");
  const a = useCustomerAnalytics(period, granularity);
  const data = a.data;
  return (
    <section className="flex flex-col gap-4" aria-label={t("analytics.title")}>
      <PeriodPicker value={period} onChange={setPeriod} today={today} />
      <ErrorText error={a.error} />
      {data && (
        <>
          <p className="text-lg">
            {t("analytics.attached", {
              share: formatBp(data.attached_bp),
              orders: data.customer_orders,
              sales: data.sales_count,
            })}
          </p>
          <div className="grid gap-6 xl:grid-cols-2">
            <div className="flex flex-col gap-2">
              <TopCustomersBar
                customers={data.top.map((c) => ({
                  id: c.customer_id,
                  label: c.display_name ?? c.phone_display ?? t("erased"),
                  visits: c.visits,
                  spend: c.spend,
                }))}
              />
              {data.top.length === 0 ? (
                <p className="text-muted-foreground">{t("empty")}</p>
              ) : (
                <table className="w-full text-left text-sm">
                  <thead className="border-b">
                    <tr>
                      <th className="py-2">{t("columns.name")}</th>
                      <th className="text-right">{t("columns.visits")}</th>
                      <th className="text-right">{t("columns.spend")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.top.map((c) => (
                      <tr key={c.customer_id} className="h-10 border-b">
                        <td>{c.display_name ?? c.phone_display ?? t("erased")}</td>
                        <td className="text-right font-mono">{c.visits}</td>
                        <td className="text-right font-mono">{formatMoney(c.spend)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
            <div className="flex flex-col gap-2">
              <div className="flex flex-wrap items-center gap-2">
                <h3 className="flex-1 text-lg font-semibold">{t("analytics.newVsReturning")}</h3>
                {(["week", "month"] as const).map((g) => (
                  <Button
                    key={g}
                    type="button"
                    size="pos"
                    variant={granularity === g ? "default" : "outline"}
                    aria-pressed={granularity === g}
                    onClick={() => setGranularity(g)}
                  >
                    {t(`analytics.by.${g}`)}
                  </Button>
                ))}
              </div>
              {data.trend.length === 0 ? (
                <p className="text-muted-foreground">{t("empty")}</p>
              ) : (
                <table className="w-full text-left text-sm">
                  <thead className="border-b">
                    <tr>
                      <th className="py-2">{t(`analytics.bucket.${granularity}`)}</th>
                      <th className="text-right">{t("analytics.new")}</th>
                      <th className="text-right">{t("analytics.returning")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.trend.map((b) => (
                      <tr key={b.bucket} className="h-10 border-b">
                        <td className="font-mono">{b.bucket}</td>
                        <td className="text-right font-mono">{b.new_customers}</td>
                        <td className="text-right font-mono">{b.returning_customers}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          </div>
        </>
      )}
    </section>
  );
}

export function CustomersScreen() {
  const { t } = useTranslation("customers");
  const [tab, setTab] = useState<Tab>("list");
  const [open, setOpen] = useState<string | null>(null);
  const today = useBusinessToday();
  return (
    <section className="flex flex-col gap-4">
      <header className="flex flex-wrap items-center gap-3">
        <h1 className="flex-1 text-2xl font-semibold">{t("title")}</h1>
        <ExportButton kind="customers" period={{ kind: "all_time" }} />
        <div className="flex gap-2" role="tablist" aria-label={t("title")}>
          <Button
            type="button"
            role="tab"
            size="pos"
            aria-selected={tab === "list"}
            variant={tab === "list" ? "default" : "outline"}
            onClick={() => setTab("list")}
          >
            <Users aria-hidden />
            {t("tabs.list")}
          </Button>
          <Button
            type="button"
            role="tab"
            size="pos"
            aria-selected={tab === "analytics"}
            variant={tab === "analytics" ? "default" : "outline"}
            onClick={() => setTab("analytics")}
          >
            <BarChart3 aria-hidden />
            {t("tabs.analytics")}
          </Button>
        </div>
      </header>
      {tab === "list" ? (
        <div className="grid gap-4 xl:grid-cols-[1fr_32rem]">
          <CustomerList onOpen={setOpen} />
          {open && <Profile key={open} id={open} onClose={() => setOpen(null)} />}
        </div>
      ) : (
        today.data && <Analytics today={today.data} />
      )}
    </section>
  );
}
