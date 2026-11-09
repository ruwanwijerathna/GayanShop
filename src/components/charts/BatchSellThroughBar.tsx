// Batch sell-through (doc §1.4): per received batch, units sold, written off and still on hand (stacked),
// with sell-through % in the table. From `batch_item_stock` via `report_batch_sell_through`.
import { useTranslation } from "react-i18next";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
} from "@/components/ui/chart";
import { ChartFigure } from "./ChartFigure";
import { percentBp, tokenConfig } from "./format";

export interface BatchUnits {
  batch_id: string;
  code: string;
  received: number;
  sold: number;
  written_off: number;
  on_hand: number;
  sell_through_bp: number;
}

export function BatchSellThroughBar({ batches }: { batches: BatchUnits[] }) {
  const { t } = useTranslation("charts");
  const config = tokenConfig({
    sold: { label: t("sold"), n: 1 },
    written_off: { label: t("writtenOff"), n: 3 },
    left: { label: t("left"), n: 5 },
  });
  // Oversold layers can make on_hand negative; the bar shows nothing left rather than a negative stack.
  const data = batches.map((b) => ({
    code: b.code,
    sold: b.sold,
    written_off: b.written_off,
    left: Math.max(b.on_hand, 0),
  }));
  const table = {
    columns: [t("batch"), t("received"), t("sold"), t("writtenOff"), t("left"), t("sellThrough")],
    rows: batches.map((b) => ({
      key: b.batch_id,
      cells: [
        b.code,
        String(b.received),
        String(b.sold),
        String(b.written_off),
        String(b.on_hand),
        percentBp(b.sell_through_bp),
      ],
    })),
  };
  return (
    <ChartFigure title={t("batchSellThrough")} table={table}>
      <ChartContainer config={config} className="aspect-auto h-64 w-full">
        <BarChart data={data} accessibilityLayer>
          <CartesianGrid vertical={false} />
          <XAxis dataKey="code" tickLine={false} axisLine={false} />
          <YAxis allowDecimals={false} tickLine={false} axisLine={false} width={48} />
          <ChartTooltip content={<ChartTooltipContent />} />
          <ChartLegend content={<ChartLegendContent />} />
          <Bar dataKey="sold" stackId="units" fill="var(--color-sold)" isAnimationActive={false} />
          <Bar
            dataKey="written_off"
            stackId="units"
            fill="var(--color-written_off)"
            isAnimationActive={false}
          />
          <Bar
            dataKey="left"
            stackId="units"
            fill="var(--color-left)"
            radius={[4, 4, 0, 0]}
            isAnimationActive={false}
          />
        </BarChart>
      </ChartContainer>
    </ChartFigure>
  );
}
