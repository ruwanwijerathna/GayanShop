import { useVirtualizer } from "@tanstack/react-virtual";
import { Minus, Plus, Trash2 } from "lucide-react";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { formatMoney } from "@/lib/money";
import { cn } from "@/lib/utils";
import type { PricedOrder } from "../lib/pricing";
import { cartStore, useCart, type CartLine } from "../store/cart.store";
import { WithTooltip } from "@/components/common/WithTooltip";

const ROW = 64;

/**
 * The cart (doc §1.9): virtualised 64 px lines, newest at the bottom with auto-scroll. A scanned or bumped
 * line flashes emerald and its qty bumps (transform/background only; off under reduced motion).
 */
export function Cart({
  priced,
  onEditLine,
}: {
  priced: PricedOrder | null;
  onEditLine?: (line: CartLine) => void;
}) {
  "use no memo";
  const { t } = useTranslation("pos");
  const lines = useCart((s) => s.lines);
  const selected = useCart((s) => s.selectedKey);
  const touch = useCart((s) => s.touch);
  const scrollRef = useRef<HTMLDivElement>(null);
  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual is not React-Compiler-safe; opted out above
  const virtualizer = useVirtualizer({
    count: lines.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW,
    overscan: 8,
    initialRect: { width: 0, height: 480 },
  });

  useEffect(() => {
    if (!touch) return;
    const index = cartStore.getState().lines.findIndex((l) => l.key === touch.key);
    if (index >= 0) virtualizer.scrollToIndex(index, { align: "end" });
  }, [touch, virtualizer]);

  useEffect(() => {
    if (!selected) return;
    const index = lines.findIndex((l) => l.key === selected);
    if (index >= 0) virtualizer.scrollToIndex(index, { align: "auto" });
  }, [selected, lines, virtualizer]);

  if (lines.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center p-6 text-center text-lg text-muted-foreground">
        {t("cart.empty")}
      </div>
    );
  }

  return (
    <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto" role="list" aria-label={t("cart.title")}>
      <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((v) => {
          const line = lines[v.index];
          if (!line) return null;
          return (
            <CartRow
              key={`${line.key}:${touch?.key === line.key ? touch.seq : 0}`}
              line={line}
              total={priced?.lines[v.index]?.line_total ?? line.unitPrice * line.qty - line.lineDiscount}
              billShare={priced?.lines[v.index]?.order_discount_share ?? 0}
              selected={selected === line.key}
              flash={touch?.key === line.key}
              top={v.start}
              onEditLine={onEditLine}
            />
          );
        })}
      </div>
    </div>
  );
}

function CartRow(props: {
  line: CartLine;
  total: number;
  billShare: number;
  selected: boolean;
  flash: boolean;
  top: number;
  onEditLine?: (line: CartLine) => void;
}) {
  const { t } = useTranslation("pos");
  const { line } = props;
  const s = cartStore.getState();
  const discount = line.lineDiscount + props.billShare;
  return (
    <div
      role="listitem"
      className={cn(
        "absolute left-0 flex w-full items-center gap-2 border-b px-3",
        props.selected && "bg-accent",
        props.flash && "animate-row-flash",
      )}
      style={{ height: ROW, transform: `translateY(${props.top}px)` }}
    >
      <button
        type="button"
        className="flex min-h-12 min-w-0 flex-1 flex-col items-start justify-center text-left"
        onClick={() => {
          s.select(line.key);
          props.onEditLine?.(line);
        }}
        aria-label={t("cart.select", { name: line.name })}
        aria-pressed={props.selected}
      >
        <span className="w-full truncate text-lg font-medium">{line.name}</span>
        <span className="w-full truncate text-sm text-muted-foreground">
          {line.productId === null && `${t("cart.quickSale")} · `}
          {t("cart.each", { price: formatMoney(line.unitPrice) })}
          {line.unitPrice < line.listPrice && ` · ${t("cart.was", { price: formatMoney(line.listPrice) })}`}
          {discount > 0 && (
            <span className="font-semibold text-emerald-600 dark:text-emerald-400">
              {` · ${t("cart.discount", { amount: formatMoney(discount) })}`}
            </span>
          )}
        </span>
      </button>
      <WithTooltip label={t("cart.less", { name: line.name })}>
        <Button
          type="button"
          variant="outline"
          size="icon-pos"
          className="shrink-0"
          aria-label={t("cart.less", { name: line.name })}
          onClick={() => (line.qty > 1 ? s.increment(line.key, -1) : s.removeLine(line.key))}
        >
          <Minus aria-hidden />
        </Button>
      </WithTooltip>
      <span
        className={cn(
          "w-12 shrink-0 text-center text-lg font-semibold tabular-nums",
          props.flash && "animate-qty-bump",
        )}
        aria-label={t("cart.qty")}
      >
        {line.qty}
      </span>
      <WithTooltip label={t("cart.more", { name: line.name })}>
        <Button
          type="button"
          variant="outline"
          size="icon-pos"
          className="shrink-0"
          aria-label={t("cart.more", { name: line.name })}
          onClick={() => s.increment(line.key, 1)}
        >
          <Plus aria-hidden />
        </Button>
      </WithTooltip>
      <span className="min-w-32 shrink-0 text-right text-lg font-semibold tabular-nums whitespace-nowrap">
        {formatMoney(props.total)}
      </span>
      <WithTooltip label={t("cart.remove", { name: line.name })}>
        <Button
          type="button"
          variant="ghost"
          size="icon-pos"
          className="shrink-0 text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
          aria-label={t("cart.remove", { name: line.name })}
          onClick={() => s.removeLine(line.key)}
        >
          <Trash2 aria-hidden className="size-5" />
        </Button>
      </WithTooltip>
    </div>
  );
}
