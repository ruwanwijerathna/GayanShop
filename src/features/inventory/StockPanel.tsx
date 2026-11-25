// Stock valuation for the Products & Stock drawer (doc §3.1, §5): value now or as of a date, by category or
// by batch with totals, the potential margin at today's prices, plus the oversold layers. Counts, damage and restocks live on each product row; movement history too.
import { Scale, TriangleAlert } from "lucide-react";
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "@/app/session";
import { StockErrorText } from "@/components/stock/useApprovedStock";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { Perm, ValuationGroup } from "@/db/bindings";
import { useOversoldLayers, useStockValuation } from "@/db/hooks";
import { formatMoney } from "@/lib/money";
import { hasPerm } from "@/lib/permissions";
import { PosTabsList, PosTabsTrigger } from "@/components/common/PosTabs";
import { Tabs, TabsContent } from "@/components/ui/tabs";

const FIELD = "h-12 text-base";
const NUM = "text-right font-mono tabular-nums whitespace-nowrap";
export type StockSection = "valuation" | "oversold";
type Breakdown = "category" | "batch";

/** `2_550` bp → `25.5%` (negative margins keep their sign). */
function formatBp(bp: number): string {
  const sign = bp < 0 ? "-" : "";
  const abs = Math.abs(bp);
  return `${sign}${Math.trunc(abs / 100)}.${Math.trunc((abs % 100) / 10)}%`;
}

/** What selling the stock at today's prices would leave over its cost, as a share of retail (basis points). */
function marginBp(cost: number, retail: number): number | null {
  return retail > 0 ? Math.round(((retail - cost) / retail) * 10_000) : null;
}

function Kpi({
  label,
  value,
  hint,
  testId,
}: {
  label: string;
  value: string;
  hint?: string;
  testId?: string;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1 rounded-lg border bg-card p-4">
      <dt className="text-sm text-muted-foreground">{label}</dt>
      <dd
        className="font-mono text-xl font-semibold tracking-tight whitespace-nowrap tabular-nums"
        data-testid={testId}
      >
        {value}
      </dd>
      {hint && <dd className="text-sm text-muted-foreground">{hint}</dd>}
    </div>
  );
}

interface Totals {
  units: number;
  value_at_cost: number;
  value_at_retail: number;
}

function GroupTable({
  title,
  rows,
  total,
  fallback,
}: {
  title: string;
  rows: ValuationGroup[];
  total: Totals;
  fallback: string;
}) {
  const { t } = useTranslation("inventory");
  return (
    <div className="overflow-x-auto rounded-lg border">
      <table className="w-full text-left text-sm" aria-label={title}>
        <thead className="bg-muted/50 text-muted-foreground">
          <tr className="h-10">
            <th className="px-3 font-medium">{title}</th>
            <th className={`px-3 font-medium ${NUM}`}>{t("valuation.units")}</th>
            <th className={`px-3 font-medium ${NUM}`}>{t("valuation.atCost")}</th>
            <th className={`px-3 font-medium ${NUM}`}>{t("valuation.atRetail")}</th>
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 ? (
            <tr className="h-12 border-t">
              <td colSpan={4} className="px-3 text-muted-foreground">
                {t("valuation.empty")}
              </td>
            </tr>
          ) : (
            rows.map((g) => (
              <tr key={g.id ?? "none"} className="h-12 border-t">
                <td className="px-3">{g.label || fallback}</td>
                <td className={`px-3 ${NUM}`}>{g.units}</td>
                <td className={`px-3 ${NUM}`}>{formatMoney(g.value_at_cost)}</td>
                <td className={`px-3 ${NUM}`}>{formatMoney(g.value_at_retail)}</td>
              </tr>
            ))
          )}
        </tbody>
        <tfoot className="border-t-2 bg-muted/30 font-semibold">
          <tr className="h-12">
            <th scope="row" className="px-3">
              {t("valuation.total")}
            </th>
            <td className={`px-3 ${NUM}`}>{total.units}</td>
            <td className={`px-3 ${NUM}`}>{formatMoney(total.value_at_cost)}</td>
            <td className={`px-3 ${NUM}`}>{formatMoney(total.value_at_retail)}</td>
          </tr>
        </tfoot>
      </table>
    </div>
  );
}

function ValuationTab() {
  const { t } = useTranslation("inventory");
  const [date, setDate] = useState("");
  const [breakdown, setBreakdown] = useState<Breakdown>("category");
  const asOf = /^\d{4}-\d{2}-\d{2}$/.test(date) ? date : null;
  const v = useStockValuation(asOf);
  const d = v.data;
  const totals: Totals | null = d
    ? { units: d.total_units, value_at_cost: d.value_at_cost, value_at_retail: d.value_at_retail }
    : null;
  const bp = d ? marginBp(d.value_at_cost, d.value_at_retail) : null;
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-end gap-3">
        <div className="flex flex-col gap-2">
          <Label htmlFor="valuation-date">{t("valuation.asOf")}</Label>
          <Input
            id="valuation-date"
            type="date"
            className={`${FIELD} w-48`}
            value={date}
            onChange={(e) => setDate(e.target.value)}
          />
        </div>
        <Button type="button" size="pos" variant="outline" disabled={date === ""} onClick={() => setDate("")}>
          {t("valuation.now")}
        </Button>
      </div>
      <StockErrorText error={v.error} />
      {d && totals && (
        <>
          <dl className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <Kpi label={t("valuation.units")} value={String(d.total_units)} />
            <Kpi label={t("valuation.atCost")} value={formatMoney(d.value_at_cost)} testId="value-at-cost" />
            <Kpi label={t("valuation.atRetail")} value={formatMoney(d.value_at_retail)} />
            <Kpi
              label={t("valuation.margin")}
              value={formatMoney(d.value_at_retail - d.value_at_cost)}
              hint={bp === null ? undefined : t("valuation.marginHint", { pct: formatBp(bp) })}
              testId="potential-margin"
            />
          </dl>
          {d.negative_layers > 0 && (
            <p role="status" className="flex items-center gap-2 rounded-md bg-warning/15 px-3 py-2">
              <TriangleAlert aria-hidden className="size-5 shrink-0" />
              {t("valuation.negative", { count: d.negative_layers })}
            </p>
          )}
          <Tabs
            value={breakdown}
            onValueChange={(x) => setBreakdown(x === "batch" ? "batch" : "category")}
            className="gap-3"
          >
            <PosTabsList aria-label={t("valuation.breakdown")}>
              <PosTabsTrigger value="category">{t("valuation.byCategory")}</PosTabsTrigger>
              <PosTabsTrigger value="batch">{t("valuation.byBatch")}</PosTabsTrigger>
            </PosTabsList>
            <TabsContent value="category">
              <GroupTable
                title={t("valuation.byCategory")}
                rows={d.by_category}
                total={totals}
                fallback={t("valuation.uncategorised")}
              />
            </TabsContent>
            <TabsContent value="batch">
              <GroupTable title={t("valuation.byBatch")} rows={d.by_batch} total={totals} fallback="" />
            </TabsContent>
          </Tabs>
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
      <div className="overflow-x-auto rounded-lg border">
        <table className="w-full text-left text-sm">
          <thead className="bg-muted/50 text-muted-foreground">
            <tr className="h-10">
              <th className="px-3 font-medium">{t("oversold.product")}</th>
              <th className="px-3 font-medium">{t("oversold.batch")}</th>
              <th className={`px-3 font-medium ${NUM}`}>{t("oversold.onHand")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.batch_item_id} className="h-12 border-t">
                <td className="px-3">{r.product_name}</td>
                <td className="px-3 font-mono whitespace-nowrap">{r.batch_code}</td>
                <td className={`px-3 text-destructive ${NUM}`}>{r.on_hand}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

const TABS: { id: StockSection; icon: ReactNode; perm: Perm }[] = [
  { id: "valuation", icon: <Scale aria-hidden />, perm: "REPORT_FINANCIAL" },
  { id: "oversold", icon: <TriangleAlert aria-hidden />, perm: "REPORT_FINANCIAL" },
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
