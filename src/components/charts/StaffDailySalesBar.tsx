// One salesperson's daily sales (Staff → Sales performance): a bar per business day, oldest first, with the
// day's order count in the tooltip and the hidden table. Rust sends every day of the window, quiet days as 0.
import { useTranslation } from "react-i18next";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { formatMoney } from "@/lib/money";
import { ChartFigure } from "./ChartFigure";
import { axisMoney, bucketLabel, tokenConfig, tooltipMoney } from "./format";

export interface DailySalesValue {
  date: string;
  total: number;
  orders: number;
}

export function StaffDailySalesBar({ days }: { days: DailySalesValue[] }) {
  const { t } = useTranslation("charts");
  const config = tokenConfig({ total: { label: t("sales"), n: 1 } });
  const data = days.map((d) => ({ day: bucketLabel(d.date), total: d.total, orders: d.orders }));
  const table = {
    columns: [t("date"), t("sales"), t("orders")],
    rows: days.map((d) => ({
      key: d.date,
      cells: [bucketLabel(d.date), formatMoney(d.total), String(d.orders)],
    })),
  };
  return (
    <ChartFigure title={t("staffDailySales")} table={table}>
      <ChartContainer config={config} className="aspect-auto h-64 w-full">
        <BarChart data={data} accessibilityLayer>
          <CartesianGrid vertical={false} />
          <XAxis dataKey="day" tickLine={false} axisLine={false} minTickGap={20} />
          <YAxis tickFormatter={axisMoney} width={88} tickLine={false} axisLine={false} />
          <ChartTooltip
            content={
              <ChartTooltipContent
                formatter={(v, _name, item) => {
                  const orders: unknown = item.payload?.orders;
                  return `${tooltipMoney(v)} · ${t("ordersCount", { count: typeof orders === "number" ? orders : 0 })}`;
                }}
              />
            }
          />
          <Bar dataKey="total" fill="var(--color-total)" radius={[4, 4, 0, 0]} isAnimationActive={false} />
        </BarChart>
      </ChartContainer>
    </ChartFigure>
  );
}
