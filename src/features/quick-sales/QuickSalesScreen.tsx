// Quick-sale review (doc §3.2, `QUICK_SALE_PROMOTE`): every quick-sale key, most sold first, with its prices
// and dates. Open one to see its median price, which counters sell it and how it was spelled; then promote
// it to a real product (optionally with a shelf count, in a received batch) or ignore it.
import { Ban, PackagePlus } from "lucide-react";
import type { ColumnDef } from "@tanstack/react-table";
import { useMemo, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { DataTable } from "@/components/data-table/DataTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { QuickSaleRow, QuickSaleStatus } from "@/db/bindings";
import { CommandError } from "@/db/client";
import {
  useBatches,
  useCategories,
  useIgnoreQuickSale,
  usePromoteQuickSale,
  useQuickSaleDetail,
  useQuickSaleReview,
} from "@/db/hooks";
import { translateError } from "@/i18n";
import { formatMoney, parseMajor, toMajorInput } from "@/lib/money";
import { SelectField } from "@/components/common/SelectField";
import { Checkbox } from "@/components/ui/checkbox";

const FIELD = "h-12 text-base";
const FILTERS: (QuickSaleStatus | null)[] = ["new", null, "promoted", "ignored"];

function Detail({ row, onDone }: { row: QuickSaleRow; onDone: () => void }) {
  const { t, i18n } = useTranslation("quickSales");
  const detail = useQuickSaleDetail(row.custom_key);
  const categories = useCategories();
  const batches = useBatches();
  const promote = usePromoteQuickSale();
  const ignore = useIgnoreQuickSale();
  const [name, setName] = useState(row.display_name);
  const [price, setPrice] = useState(toMajorInput(row.last_price));
  const [cost, setCost] = useState("");
  const [categoryId, setCategoryId] = useState("");
  const [barcode, setBarcode] = useState(true);
  const [shelf, setShelf] = useState("");
  const [batchId, setBatchId] = useState("");

  const priceN = parseMajor(price);
  const costN = parseMajor(cost);
  const shelfN =
    shelf.trim() === "" ? null : /^\d{1,7}$/.test(shelf.trim()) ? Number.parseInt(shelf.trim(), 10) : -1;
  const valid = name.trim() !== "" && priceN !== null && costN !== null && shelfN !== -1;
  const received = (batches.data ?? []).filter((b) => b.status === "received" && !b.is_system);
  const error = [promote.error, ignore.error].find((e) => e instanceof CommandError);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!valid || priceN === null || costN === null) return;
    promote.mutate(
      {
        custom_key: row.custom_key,
        name: name.trim(),
        price: priceN,
        default_cost: costN,
        category_id: categoryId || null,
        generate_barcode: barcode,
        shelf_qty: shelfN,
        batch_id: shelfN !== null && batchId ? batchId : null,
      },
      {
        onSuccess: (r) => {
          toast.success(t("promoted", { name: r.product.name }));
          onDone();
        },
      },
    );
  };

  return (
    <section className="flex flex-col gap-4 rounded-lg border p-4" aria-label={row.display_name}>
      <h2 className="text-xl font-semibold">{row.display_name}</h2>
      {detail.data && (
        <dl className="grid grid-cols-2 gap-x-6 gap-y-1 text-sm">
          <dt className="text-muted-foreground">{t("detail.median")}</dt>
          <dd className="font-mono">{formatMoney(detail.data.median_price)}</dd>
          <dt className="text-muted-foreground">{t("detail.counters")}</dt>
          <dd>
            {detail.data.counters
              .map((c) => t("detail.counter", { code: c.device_code, count: c.times_sold }))
              .join(", ")}
          </dd>
          <dt className="text-muted-foreground">{t("detail.spellings")}</dt>
          <dd>{detail.data.spellings.join(" / ")}</dd>
        </dl>
      )}
      {row.status === "promoted" ? (
        <p className="text-base">{t("alreadyPromoted")}</p>
      ) : (
        <form onSubmit={submit} className="grid gap-3 sm:grid-cols-2" aria-label={t("promote.title")}>
          <div className="flex flex-col gap-2 sm:col-span-2">
            <Label htmlFor="qs-name">{t("promote.name")}</Label>
            <Input
              id="qs-name"
              className={FIELD}
              maxLength={120}
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="qs-price">{t("promote.price")}</Label>
            <Input
              id="qs-price"
              className={FIELD}
              inputMode="decimal"
              value={price}
              onChange={(e) => setPrice(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="qs-cost">{t("promote.cost")}</Label>
            <Input
              id="qs-cost"
              className={FIELD}
              inputMode="decimal"
              value={cost}
              onChange={(e) => setCost(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="qs-category">{t("promote.category")}</Label>
            <SelectField
              id="qs-category"
              value={categoryId}
              onValueChange={setCategoryId}
              options={[
                { value: "", label: t("promote.noCategory") },
                ...(categories.data ?? []).map((c) => ({ value: c.id, label: c.name })),
              ]}
            />
          </div>
          <label className="flex min-h-12 items-center gap-3 self-end text-base">
            <Checkbox className="size-6" checked={barcode} onCheckedChange={(v) => setBarcode(v === true)} />
            {t("promote.barcode")}
          </label>
          <div className="flex flex-col gap-2">
            <Label htmlFor="qs-shelf">{t("promote.shelf")}</Label>
            <Input
              id="qs-shelf"
              className={FIELD}
              inputMode="numeric"
              value={shelf}
              onChange={(e) => setShelf(e.target.value)}
              aria-invalid={shelfN === -1}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="qs-batch">{t("promote.batch")}</Label>
            <SelectField
              id="qs-batch"
              value={batchId}
              onValueChange={setBatchId}
              disabled={shelfN === null}
              options={[
                { value: "", label: t("promote.noBatch") },
                ...received.map((b) => ({ value: b.id, label: `${b.code} · ${b.name}` })),
              ]}
            />
          </div>
          {error instanceof CommandError && (
            <p role="alert" className="text-base text-destructive sm:col-span-2">
              {translateError(i18n.t, error.error)}
            </p>
          )}
          <div className="flex flex-wrap gap-2 sm:col-span-2">
            <Button type="submit" size="pos" disabled={!valid || promote.isPending}>
              <PackagePlus aria-hidden />
              {t("promote.submit")}
            </Button>
            {row.status === "new" && (
              <Button
                type="button"
                size="pos"
                variant="outline"
                disabled={ignore.isPending}
                onClick={() => ignore.mutate(row.custom_key, { onSuccess: onDone })}
              >
                <Ban aria-hidden />
                {t("ignore")}
              </Button>
            )}
          </div>
        </form>
      )}
    </section>
  );
}

export function QuickSalesScreen() {
  const { t } = useTranslation(["quickSales", "common"]);
  const [show, setShow] = useState<QuickSaleStatus | null>("new");
  const review = useQuickSaleReview(show);
  const [open, setOpen] = useState<QuickSaleRow | null>(null);
  const rows = review.data ?? [];
  const columns = useMemo<ColumnDef<QuickSaleRow>[]>(
    () => [
      {
        id: "name",
        header: t("columns.name"),
        size: 220,
        enableSorting: false,
        cell: ({ row }) => (
          <button
            type="button"
            className="min-h-12 text-left font-medium underline-offset-4 hover:underline"
            onClick={() => setOpen(row.original)}
          >
            {row.original.display_name}
          </button>
        ),
      },
      { id: "times", header: t("columns.times"), size: 80, accessorFn: (r) => r.times_sold },
      { id: "units", header: t("columns.units"), size: 80, accessorFn: (r) => r.units_sold },
      {
        id: "revenue",
        header: t("columns.revenue"),
        size: 130,
        accessorFn: (r) => r.revenue_net,
        cell: ({ row }) => formatMoney(row.original.revenue_net),
      },
      {
        id: "prices",
        header: t("columns.prices"),
        size: 200,
        enableSorting: false,
        cell: ({ row }) => `${formatMoney(row.original.min_price)}–${formatMoney(row.original.max_price)}`,
      },
      {
        id: "last",
        header: t("columns.last"),
        size: 120,
        accessorFn: (r) => r.last_sold_at,
        cell: ({ row }) => new Date(row.original.last_sold_at).toLocaleDateString(),
      },
      {
        id: "status",
        header: t("columns.status"),
        size: 120,
        enableSorting: false,
        cell: ({ row }) => (
          <Badge variant={row.original.status === "new" ? "outline" : "secondary"}>
            {t(`status.${row.original.status}`)}
          </Badge>
        ),
      },
    ],
    [t],
  );

  return (
    <section className="flex flex-col gap-4">
      <header className="flex flex-wrap items-center gap-3">
        <h1 className="flex-1 text-2xl font-semibold">{t("title")}</h1>
        <div className="flex flex-wrap gap-2" role="group" aria-label={t("filter")}>
          {FILTERS.map((f) => (
            <Button
              key={f ?? "all"}
              type="button"
              size="pos"
              variant={show === f ? "default" : "outline"}
              aria-pressed={show === f}
              onClick={() => {
                setShow(f);
                setOpen(null);
              }}
            >
              {t(`filters.${f ?? "all"}`)}
            </Button>
          ))}
        </div>
      </header>
      <p className="text-sm text-muted-foreground">{t("hint")}</p>
      <div className="grid gap-4 xl:grid-cols-[1fr_28rem]">
        <DataTable<QuickSaleRow>
          ariaLabel={t("title")}
          columns={columns}
          data={rows}
          getRowId={(r) => r.custom_key}
          height={480}
          emptyLabel={review.isLoading ? t("common:table.loading") : t("empty")}
          onRowActivate={setOpen}
        />
        {open && <Detail key={open.custom_key} row={open} onDone={() => setOpen(null)} />}
      </div>
    </section>
  );
}
