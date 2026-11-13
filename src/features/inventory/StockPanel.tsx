// Stock valuation for the Products & Stock drawer (doc §3.1, §5): value now or as of a date, by category,
// plus the oversold layers. Counts, damage and restocks live on each product row; movement history too.
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
export type StockSection = "valuation" | "oversold";

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
      <StockErrorText error={v.error} />
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
