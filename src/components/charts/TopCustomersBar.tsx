// Customer spend, top N (doc §1.4): horizontal bars from customer analytics (`customer_stats` for all time).
import { useTranslation } from "react-i18next";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { formatMoney } from "@/lib/money";
import { ChartFigure } from "./ChartFigure";
import { axisMoney, tokenConfig, tooltipMoney } from "./format";

export interface CustomerSpend {
  id: string;
  label: string;
  visits: number;
  spend: number;
}

export function TopCustomersBar({ customers }: { customers: CustomerSpend[] }) {
  const { t } = useTranslation("charts");
  const config = tokenConfig({ spend: { label: t("spend"), n: 2 } });
  const table = {
    columns: [t("customer"), t("visits"), t("spend")],
    rows: customers.map((c) => ({ key: c.id, cells: [c.label, String(c.visits), formatMoney(c.spend)] })),
  };
  return (
    <ChartFigure title={t("topCustomers")} table={table}>
      <ChartContainer
        config={config}
        className="aspect-auto w-full"
        style={{ height: 48 + customers.length * 36 }}
      >
        <BarChart data={customers} layout="vertical" margin={{ left: 8 }} accessibilityLayer>
          <CartesianGrid horizontal={false} />
          <XAxis type="number" dataKey="spend" tickFormatter={axisMoney} tickLine={false} axisLine={false} />
          <YAxis type="category" dataKey="label" width={128} tickLine={false} axisLine={false} />
          <ChartTooltip
            content={<ChartTooltipContent formatter={(v) => `${t("spend")}: ${tooltipMoney(v)}`} />}
          />
          <Bar dataKey="spend" fill="var(--color-spend)" radius={4} isAnimationActive={false} />
        </BarChart>
      </ChartContainer>
    </ChartFigure>
  );
}
