// Stock tools for the Products & Stock page (doc §3.1, §5): valuation now or as of a date (by batch and by category), oversold layers, a stock
// count, damage and adjustments (with a supervisor's approval where the role lacks it), and movement history.
import { ClipboardList, History, PackageMinus, Scale, TriangleAlert } from "lucide-react";
import type { ColumnDef } from "@tanstack/react-table";
import { useMemo, useState, type FormEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useSession } from "@/app/session";
import { ApprovalDialog } from "@/components/auth/ApprovalDialog";
import { DataTable } from "@/components/data-table/DataTable";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type {
  DamageReason,
  MovementRow_Serialize as MovementRow,
  Perm,
  ProductSummary,
  ValuationGroup,
} from "@/db/bindings";
import { CommandError } from "@/db/client";
import {
  useAdjustStock,
  useOversoldLayers,
  useProductSearch,
  useRecordDamage,
  useRecordStockCount,
  useStockMovements,
  useStockValuation,
} from "@/db/hooks";
import { translateError } from "@/i18n";
import { formatMoney } from "@/lib/money";
import { hasPerm } from "@/lib/permissions";
import { cn } from "@/lib/utils";
import { SelectField } from "@/components/common/SelectField";
import { PosTabsList, PosTabsTrigger } from "@/components/common/PosTabs";
import { Tabs, TabsContent } from "@/components/ui/tabs";

/** Must match `commands::inventory::STOCK_TARGET`. */
export const STOCK_TARGET = "stock";
const FIELD = "h-12 text-base";
const REASONS: DamageReason[] = ["broken", "expired", "stolen", "faulty", "other"];
export type StockSection = "valuation" | "oversold" | "count" | "damage" | "history";

/** Whole units: "12" → 12, "-3" → -3 when `signed`. */
export function parseUnits(text: string, signed = false): number | null {
  const s = text.trim();
  if (!(signed ? /^-?\d{1,7}$/ : /^\d{1,7}$/).test(s)) return null;
  return Number.parseInt(s, 10);
}

/** Search-and-pick one product (tracked stock only matters to Rust, which re-checks). */
function ProductSearch({ onPick, label }: { onPick: (p: ProductSummary) => void; label: string }) {
  const [q, setQ] = useState("");
  const results = useProductSearch(q);
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={`search-${label}`}>{label}</Label>
      <Input
        id={`search-${label}`}
        type="search"
        className={FIELD}
        value={q}
        onChange={(e) => setQ(e.target.value)}
      />
      {q.trim() !== "" && (
        <ul className="flex max-h-48 flex-col gap-1 overflow-y-auto">
          {(results.data ?? []).map((p) => (
            <li key={p.id}>
              <button
                type="button"
                className="flex min-h-12 w-full items-center gap-2 rounded-md border px-3 text-left"
                onClick={() => {
                  onPick(p);
                  setQ("");
                }}
              >
                <span className="flex-1">{p.name}</span>
                <span className="font-mono text-sm text-muted-foreground">{p.sku ?? p.barcode}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** Run a stock write; on ApprovalRequired, ask a supervisor and retry with the approval. */
function useApproved() {
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
      onCancel={() => setPending(null)}
    />
  ) : null;
  return { run, dialog };
}

function ErrorText({ error }: { error: unknown }) {
  const { i18n } = useTranslation();
  if (!(error instanceof CommandError) || error.error.kind === "APPROVAL_REQUIRED") return null;
  return (
    <p role="alert" className="text-base text-destructive">
      {translateError(i18n.t, error.error)}
    </p>
  );
}

function GroupTable({
  title,
  rows,
  uncategorised,
}: {
  title: string;
  rows: ValuationGroup[];
  uncategorised: string;
}) {
  const { t } = useTranslation("inventory");
  return (
    <table className="w-full text-left text-sm" aria-label={title}>
      <thead className="border-b">
        <tr>
          <th className="py-2">{title}</th>
          <th className="text-right">{t("valuation.units")}</th>
          <th className="text-right">{t("valuation.atCost")}</th>
          <th className="text-right">{t("valuation.atRetail")}</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((g) => (
          <tr key={g.id ?? "none"} className="h-10 border-b">
            <td>{g.label || uncategorised}</td>
            <td className="text-right font-mono">{g.units}</td>
            <td className="text-right font-mono">{formatMoney(g.value_at_cost)}</td>
            <td className="text-right font-mono">{formatMoney(g.value_at_retail)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function ValuationTab() {
  const { t } = useTranslation("inventory");
  const [date, setDate] = useState("");
  const asOf = /^\d{4}-\d{2}-\d{2}$/.test(date) ? date : null;
  const v = useStockValuation(asOf);
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-end gap-3">
        <div className="flex flex-col gap-2">
          <Label htmlFor="valuation-date">{t("valuation.asOf")}</Label>
          <Input
            id="valuation-date"
            type="date"
            className={FIELD}
            value={date}
            onChange={(e) => setDate(e.target.value)}
          />
        </div>
        {date !== "" && (
          <Button type="button" size="pos" variant="outline" onClick={() => setDate("")}>
            {t("valuation.now")}
          </Button>
        )}
      </div>
      <ErrorText error={v.error} />
      {v.data && (
        <>
          <dl className="grid grid-cols-3 gap-3">
            <div className="rounded-md border p-3">
              <dt className="text-sm text-muted-foreground">{t("valuation.units")}</dt>
              <dd className="font-mono text-xl">{v.data.total_units}</dd>
            </div>
            <div className="rounded-md border p-3">
              <dt className="text-sm text-muted-foreground">{t("valuation.atCost")}</dt>
              <dd className="font-mono text-xl" data-testid="value-at-cost">
                {formatMoney(v.data.value_at_cost)}
              </dd>
            </div>
            <div className="rounded-md border p-3">
              <dt className="text-sm text-muted-foreground">{t("valuation.atRetail")}</dt>
              <dd className="font-mono text-xl">{formatMoney(v.data.value_at_retail)}</dd>
            </div>
          </dl>
          {v.data.negative_layers > 0 && (
            <p role="status" className="flex items-center gap-2 rounded-md bg-warning/15 px-3 py-2">
              <TriangleAlert aria-hidden className="size-5" />
              {t("valuation.negative", { count: v.data.negative_layers })}
            </p>
          )}
          <GroupTable title={t("valuation.byBatch")} rows={v.data.by_batch} uncategorised="" />
          <GroupTable
            title={t("valuation.byCategory")}
            rows={v.data.by_category}
            uncategorised={t("valuation.uncategorised")}
          />
        </>
      )}
    </div>
  );
}

function OversoldTab() {
  const { t } = useTranslation("inventory");
  const layers = useOversoldLayers();
  const rows = layers.data ?? [];
  return rows.length === 0 ? (
    <p className="text-muted-foreground">{t("oversold.none")}</p>
  ) : (
    <div className="flex flex-col gap-2">
      <p className="text-sm text-muted-foreground">{t("oversold.hint")}</p>
      <table className="w-full text-left text-sm">
        <thead className="border-b">
          <tr>
            <th className="py-2">{t("oversold.product")}</th>
            <th>{t("oversold.batch")}</th>
            <th className="text-right">{t("oversold.onHand")}</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.batch_item_id} className="h-10 border-b">
              <td>{r.product_name}</td>
              <td className="font-mono">{r.batch_code}</td>
              <td className="text-right font-mono text-destructive">{r.on_hand}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function CountTab() {
  const { t } = useTranslation("inventory");
  const count = useRecordStockCount();
  const { run, dialog } = useApproved();
  const [lines, setLines] = useState<{ product: ProductSummary; counted: string }[]>([]);
  const [note, setNote] = useState("");
  const parsed = lines.map((l) => parseUnits(l.counted));
  const ok = lines.length > 0 && parsed.every((n) => n !== null);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!ok) return;
    const input = {
      lines: lines.map((l, i) => ({ product_id: l.product.id, counted: parsed[i] ?? 0 })),
      note: note.trim() || null,
    };
    run((approvalId) =>
      count.mutateAsync({ input, approvalId }).then((r) => {
        toast.success(t("count.done", { corrected: r.changes.length, unchanged: r.unchanged }));
        setLines([]);
        setNote("");
      }),
    );
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-4" aria-label={t("tabs.count")}>
      <ProductSearch
        label={t("count.add")}
        onPick={(p) =>
          setLines((ls) =>
            ls.some((l) => l.product.id === p.id) ? ls : [...ls, { product: p, counted: "" }],
          )
        }
      />
      {lines.length > 0 && (
        <ul className="flex flex-col gap-2">
          {lines.map((l, i) => (
            <li key={l.product.id} className="flex items-center gap-3">
              <span className="flex-1">{l.product.name}</span>
              <Label htmlFor={`counted-${l.product.id}`} className="sr-only">
                {t("count.counted", { name: l.product.name })}
              </Label>
              <Input
                id={`counted-${l.product.id}`}
                inputMode="numeric"
                className="h-12 w-28 text-right font-mono text-base"
                value={l.counted}
                onChange={(e) =>
                  setLines((ls) => ls.map((x, j) => (j === i ? { ...x, counted: e.target.value } : x)))
                }
                aria-invalid={parsed[i] === null}
              />
            </li>
          ))}
        </ul>
      )}
      <div className="flex flex-col gap-2">
        <Label htmlFor="count-note">{t("note")}</Label>
        <Input
          id="count-note"
          className={FIELD}
          maxLength={200}
          value={note}
          onChange={(e) => setNote(e.target.value)}
        />
      </div>
      <ErrorText error={count.error} />
      <Button type="submit" size="pos" className="self-start" disabled={!ok || count.isPending}>
        {t("count.submit", { count: lines.length })}
      </Button>
      {dialog}
    </form>
  );
}

function DamageTab() {
  const { t } = useTranslation("inventory");
  const damage = useRecordDamage();
  const adjust = useAdjustStock();
  const { run, dialog } = useApproved();
  const [product, setProduct] = useState<ProductSummary | null>(null);
  const [mode, setMode] = useState<"damage" | "adjust">("damage");
  const [qty, setQty] = useState("");
  const [reason, setReason] = useState<DamageReason>("broken");
  const [note, setNote] = useState("");
  const units = parseUnits(qty, mode === "adjust");
  const ok =
    product !== null && units !== null && units !== 0 && (mode === "damage" ? units > 0 : note.trim() !== "");

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!ok || !product || units === null) return;
    const done = () => {
      toast.success(t("damage.done", { name: product.name }));
      setQty("");
      setNote("");
    };
    if (mode === "damage") {
      const input = { product_id: product.id, qty: units, reason, note: note.trim() || null };
      run((approvalId) => damage.mutateAsync({ input, approvalId }).then(done));
    } else {
      const input = { product_id: product.id, qty_delta: units, note };
      run((approvalId) => adjust.mutateAsync({ input, approvalId }).then(done));
    }
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-4" aria-label={t("tabs.damage")}>
      <div className="flex gap-2" role="group" aria-label={t("damage.kind")}>
        {(["damage", "adjust"] as const).map((m) => (
          <Button
            key={m}
            type="button"
            size="pos"
            variant={mode === m ? "default" : "outline"}
            aria-pressed={mode === m}
            onClick={() => setMode(m)}
          >
            {t(`damage.modes.${m}`)}
          </Button>
        ))}
      </div>
      {product ? (
        <p className="flex items-center gap-3">
          <strong className="flex-1">{product.name}</strong>
          <Button type="button" size="pos" variant="outline" onClick={() => setProduct(null)}>
            {t("damage.change")}
          </Button>
        </p>
      ) : (
        <ProductSearch label={t("damage.product")} onPick={setProduct} />
      )}
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="flex flex-col gap-2">
          <Label htmlFor="stock-qty">{mode === "damage" ? t("damage.qty") : t("damage.delta")}</Label>
          <Input
            id="stock-qty"
            inputMode="numeric"
            className={FIELD}
            value={qty}
            onChange={(e) => setQty(e.target.value)}
          />
        </div>
        {mode === "damage" && (
          <div className="flex flex-col gap-2">
            <Label htmlFor="damage-reason">{t("damage.reason")}</Label>
            <SelectField
              id="damage-reason"
              value={reason}
              onValueChange={(v) => setReason(REASONS.find((r) => r === v) ?? "other")}
              options={REASONS.map((r) => ({ value: r, label: t(`damage.reasons.${r}`) }))}
            />
          </div>
        )}
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor="stock-note">
          {mode === "adjust" || reason === "other" ? t("noteRequired") : t("note")}
        </Label>
        <Input
          id="stock-note"
          className={FIELD}
          maxLength={200}
          value={note}
          onChange={(e) => setNote(e.target.value)}
        />
      </div>
      <ErrorText error={mode === "damage" ? damage.error : adjust.error} />
      <Button
        type="submit"
        size="pos"
        className="self-start"
        disabled={!ok || damage.isPending || adjust.isPending}
      >
        {mode === "damage" ? t("damage.submit") : t("damage.submitAdjust")}
      </Button>
      {dialog}
    </form>
  );
}

function HistoryTab() {
  const { t } = useTranslation(["inventory", "common"]);
  const [product, setProduct] = useState<ProductSummary | null>(null);
  const history = useStockMovements(product?.id ?? null);
  const rows = useMemo(() => history.data?.pages.flatMap((p) => p.rows) ?? [], [history.data]);
  const columns = useMemo<ColumnDef<MovementRow>[]>(
    () => [
      {
        id: "when",
        header: t("history.when"),
        size: 190,
        enableSorting: false,
        accessorFn: (r) => new Date(r.created_at).toLocaleString(),
      },
      {
        id: "reason",
        header: t("history.reason"),
        size: 260,
        enableSorting: false,
        cell: ({ row }) => (
          <span>
            {t(`history.reasons.${row.original.reason}`, { defaultValue: row.original.reason })}
            {row.original.note && <span className="ml-2 text-muted-foreground">{row.original.note}</span>}
          </span>
        ),
      },
      {
        id: "batch",
        header: t("history.batch"),
        size: 150,
        enableSorting: false,
        accessorFn: (r) => r.batch_code,
      },
      {
        id: "qty",
        header: t("history.qty"),
        size: 80,
        enableSorting: false,
        cell: ({ row }) => (
          <span className={cn("font-mono", row.original.qty_delta < 0 && "text-destructive")}>
            {row.original.qty_delta}
          </span>
        ),
      },
      {
        id: "cost",
        header: t("history.cost"),
        size: 120,
        enableSorting: false,
        accessorFn: (r) => formatMoney(r.unit_cost),
      },
      {
        id: "by",
        header: t("history.by"),
        size: 140,
        enableSorting: false,
        accessorFn: (r) => r.staff_name ?? "",
      },
    ],
    [t],
  );
  return (
    <div className="flex flex-col gap-3">
      {product ? (
        <p className="flex items-center gap-3">
          <strong className="flex-1">{product.name}</strong>
          <Button type="button" size="pos" variant="outline" onClick={() => setProduct(null)}>
            {t("damage.change")}
          </Button>
        </p>
      ) : (
        <ProductSearch label={t("history.product")} onPick={setProduct} />
      )}
      {product && (
        <DataTable<MovementRow>
          ariaLabel={t("tabs.history")}
          columns={columns}
          data={rows}
          getRowId={(r) => r.id}
          height={432}
          rowHeight={48}
          emptyLabel={history.isLoading ? t("common:table.loading") : t("history.empty")}
          onEndReached={() => {
            if (history.hasNextPage && !history.isFetchingNextPage) void history.fetchNextPage();
          }}
        />
      )}
    </div>
  );
}

const TABS: { id: StockSection; icon: ReactNode; perm: Perm }[] = [
  { id: "valuation", icon: <Scale aria-hidden />, perm: "REPORT_FINANCIAL" },
  { id: "oversold", icon: <TriangleAlert aria-hidden />, perm: "REPORT_FINANCIAL" },
  { id: "count", icon: <ClipboardList aria-hidden />, perm: "SALE_CREATE" },
  { id: "damage", icon: <PackageMinus aria-hidden />, perm: "SALE_CREATE" },
  { id: "history", icon: <History aria-hidden />, perm: "REPORT_FINANCIAL" },
];

/** Who may open a stock section (hides only; Rust enforces, asking a supervisor where needed). */
export const STOCK_SECTION_PERMS: Record<StockSection, Perm> = Object.fromEntries(
  TABS.map((x) => [x.id, x.perm]),
) as Record<StockSection, Perm>;

/**
 * Some of the stock tools, for the Products & Stock page (composed in `src/app`, since features never
 * import each other). Several sections get their own sub-tabs; a single one renders on its own. Sections
 * the staff member can't open are left out.
 */
export function StockPanel({ sections, label }: { sections: readonly StockSection[]; label: string }) {
  const { t } = useTranslation("inventory");
  const { session } = useSession();
  // In the order given: the first section the staff member may open is the one shown first.
  const visible = sections
    .map((id) => TABS.find((x) => x.id === id))
    .filter((x): x is (typeof TABS)[number] => x !== undefined && hasPerm(session, x.perm));
  const [picked, setPicked] = useState<StockSection | null>(null);
  const first = visible[0];
  if (!first) return null;
  const tab = picked && visible.some((x) => x.id === picked) ? picked : first.id;
  const body = (id: StockSection) => (
    <>
      {id === "valuation" && <ValuationTab />}
      {id === "oversold" && <OversoldTab />}
      {id === "count" && <CountTab />}
      {id === "damage" && <DamageTab />}
      {id === "history" && <HistoryTab />}
    </>
  );
  if (visible.length === 1) return body(first.id);
  return (
    <Tabs
      value={tab}
      onValueChange={(v) => setPicked(visible.find((x) => x.id === v)?.id ?? first.id)}
      className="gap-4"
    >
      <PosTabsList aria-label={label}>
        {visible.map((x) => (
          <PosTabsTrigger key={x.id} value={x.id}>
            {x.icon}
            {t(`tabs.${x.id}`)}
          </PosTabsTrigger>
        ))}
      </PosTabsList>
      {visible.map((x) => (
        <TabsContent key={x.id} value={x.id}>
          {body(x.id)}
        </TabsContent>
      ))}
    </Tabs>
  );
}
