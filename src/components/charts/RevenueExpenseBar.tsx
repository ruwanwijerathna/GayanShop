// Revenue vs expenses (doc §1.4): grouped bars per day, week or month from `report_trend`. Without cost
// permissions Rust sends no expenses, and the chart shows revenue alone.
import { useTranslation } from "react-i18next";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
} from "@/components/ui/chart";
import { formatMoney } from "@/lib/money";
import { ChartFigure } from "./ChartFigure";
import { axisMoney, bucketLabel, tokenConfig, tooltipMoney } from "./format";

export interface RevenuePoint {
  bucket: string;
  net_sales: number;
  expenses: number | null;
}

export function RevenueExpenseBar({ points }: { points: RevenuePoint[] }) {
  const { t } = useTranslation("charts");
  const withExpenses = points.some((p) => p.expenses !== null);
  const config = tokenConfig({
    revenue: { label: t("revenue"), n: 1 },
    ...(withExpenses ? { expenses: { label: t("expenses"), n: 3 } } : {}),
  });
  const data = points.map((p) => ({
    bucket: bucketLabel(p.bucket),
    revenue: p.net_sales,
    expenses: p.expenses ?? 0,
  }));
  const table = {
    columns: [t("period"), t("revenue"), ...(withExpenses ? [t("expenses")] : [])],
    rows: points.map((p) => ({
      key: p.bucket,
      cells: [
        bucketLabel(p.bucket),
        formatMoney(p.net_sales),
        ...(withExpenses ? [formatMoney(p.expenses ?? 0)] : []),
      ],
    })),
  };
  return (
    <ChartFigure title={t("revenueVsExpenses")} table={table}>
      <ChartContainer config={config} className="aspect-auto h-64 w-full">
        <BarChart data={data} accessibilityLayer>
          <CartesianGrid vertical={false} />
          <XAxis dataKey="bucket" tickLine={false} axisLine={false} minTickGap={16} />
          <YAxis tickFormatter={axisMoney} width={88} tickLine={false} axisLine={false} />
          <ChartTooltip
            content={
              <ChartTooltipContent
                formatter={(v, name) => `${String(config[String(name)]?.label ?? name)}: ${tooltipMoney(v)}`}
              />
            }
          />
          <ChartLegend content={<ChartLegendContent />} />
          <Bar dataKey="revenue" fill="var(--color-revenue)" radius={4} />
          {withExpenses && <Bar dataKey="expenses" fill="var(--color-expenses)" radius={4} />}
        </BarChart>
      </ChartContainer>
    </ChartFigure>
  );
}
