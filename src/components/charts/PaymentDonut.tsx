// Payment methods (doc §1.4): a donut of Σ payments by method for the period, total in the middle.
import { useTranslation } from "react-i18next";
import { Cell, Label, Pie, PieChart } from "recharts";
import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
} from "@/components/ui/chart";
import { formatMoney } from "@/lib/money";
import { ChartFigure } from "./ChartFigure";
import { percentBp, tokenConfig, tooltipMoney } from "./format";

export interface MethodSlice {
  method: string;
  amount: number;
  count: number;
}

const ORDER: Record<string, 1 | 2 | 3 | 4 | 5> = { cash: 1, card: 2, bank_transfer: 3, qr: 4, other: 5 };

export function PaymentDonut({ payments }: { payments: MethodSlice[] }) {
  const { t } = useTranslation("charts");
  const slices = payments.filter((p) => p.amount > 0);
  const total = slices.reduce((s, p) => s + p.amount, 0);
  const config = tokenConfig(
    Object.fromEntries(
      slices.map((p) => [
        p.method,
        { label: t(`methods.${p.method}`, { defaultValue: p.method }), n: ORDER[p.method] ?? 5 },
      ]),
    ),
  );
  const table = {
    columns: [t("method"), t("amount"), t("share"), t("count")],
    rows: slices.map((p) => ({
      key: p.method,
      cells: [
        String(config[p.method]?.label ?? p.method),
        formatMoney(p.amount),
        percentBp(total > 0 ? Math.trunc((p.amount * 10_000) / total) : 0),
        String(p.count),
      ],
    })),
  };
  return (
    <ChartFigure title={t("paymentMethods")} table={table}>
      <ChartContainer config={config} className="mx-auto aspect-square h-64">
        <PieChart accessibilityLayer>
          <ChartTooltip
            content={
              <ChartTooltipContent
                nameKey="method"
                hideLabel
                formatter={(v, name) => `${String(config[String(name)]?.label ?? name)}: ${tooltipMoney(v)}`}
              />
            }
          />
          <Pie
            data={slices}
            dataKey="amount"
            nameKey="method"
            innerRadius="58%"
            strokeWidth={2}
            isAnimationActive={false}
          >
            {slices.map((p) => (
              <Cell key={p.method} fill={`var(--color-${p.method})`} />
            ))}
            <Label
              position="center"
              className="fill-foreground text-sm font-semibold"
              value={formatMoney(total)}
            />
          </Pie>
          <ChartLegend content={<ChartLegendContent nameKey="method" />} />
        </PieChart>
      </ChartContainer>
    </ChartFigure>
  );
}
