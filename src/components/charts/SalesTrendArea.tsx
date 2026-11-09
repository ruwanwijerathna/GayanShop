// Sales trend (doc §1.4): net sales per bucket as an area, with the previous equivalent period as a dashed
// overlay. Rust sends both series densified, so point i of each lines up.
import { useTranslation } from "react-i18next";
import { Area, AreaChart, CartesianGrid, XAxis, YAxis } from "recharts";
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

export interface TrendValue {
  bucket: string;
  net_sales: number;
}

/** Pair the current and previous series point by point (the overlay is shorter when months differ). */
export function overlay(current: TrendValue[], previous: TrendValue[] | null) {
  return current.map((p, i) => ({
    bucket: bucketLabel(p.bucket),
    current: p.net_sales,
    previous: previous ? (previous[i]?.net_sales ?? null) : null,
  }));
}

export function SalesTrendArea({
  current,
  previous,
}: {
  current: TrendValue[];
  previous: TrendValue[] | null;
}) {
  const { t } = useTranslation("charts");
  const config = tokenConfig({
    current: { label: t("thisPeriod"), n: 1 },
    ...(previous ? { previous: { label: t("previousPeriod"), n: 5 } } : {}),
  });
  const data = overlay(current, previous);
  const table = {
    columns: [t("period"), t("thisPeriod"), ...(previous ? [t("previousPeriod")] : [])],
    rows: data.map((d, i) => ({
      key: current[i]?.bucket ?? String(i),
      cells: [
        d.bucket,
        formatMoney(d.current),
        ...(previous ? [d.previous === null ? "—" : formatMoney(d.previous)] : []),
      ],
    })),
  };
  return (
    <ChartFigure title={t("salesTrend")} table={table}>
      <ChartContainer config={config} className="aspect-auto h-64 w-full">
        <AreaChart data={data} accessibilityLayer>
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
          {previous && (
            <Area
              dataKey="previous"
              type="monotone"
              stroke="var(--color-previous)"
              strokeDasharray="6 4"
              fill="none"
              isAnimationActive={false}
            />
          )}
          <Area
            dataKey="current"
            type="monotone"
            stroke="var(--color-current)"
            fill="var(--color-current)"
            fillOpacity={0.25}
            isAnimationActive={false}
          />
        </AreaChart>
      </ChartContainer>
    </ChartFigure>
  );
}
