import {
  ArrowDownToLine,
  ArrowUpFromLine,
  ClipboardCheck,
  History,
  Loader2,
  PackageCheck,
  PackageX,
  RotateCcw,
  Scale,
  ShoppingCart,
  SlidersHorizontal,
  Undo2,
  type LucideIcon,
} from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import type { MovementRow_Serialize as MovementRow, Product } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useStockMovements } from "@/db/hooks";
import { translateError } from "@/i18n";
import { formatMoney } from "@/lib/money";
import { cn } from "@/lib/utils";

const REASON_ICONS: Record<string, LucideIcon> = {
  batch_intake: PackageCheck,
  batch_reopen: RotateCcw,
  sale: ShoppingCart,
  void: Undo2,
  damage: PackageX,
  count_correction: ClipboardCheck,
  adjustment: SlidersHorizontal,
};

const signed = (n: number) => (n > 0 ? `+${n}` : String(n));

type Entry = { row: MovementRow; balance: number | null };
type Day = { date: string; entries: Entry[]; net: number };

/** `YYYY-MM-DD` as a local calendar date (not UTC midnight, which can land on the previous day). */
function businessDay(date: string): Date {
  const [y, m, d] = date.split("-").map((x) => Number.parseInt(x, 10));
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1);
}

/**
 * A product's stock ledger as a timeline (newest first): totals in and out, a filter by type, and per day
 * every movement with its batch, unit cost, who did it, who approved it and the balance it left behind.
 */
export function MovementHistorySheet({ product, onClose }: { product: Product; onClose: () => void }) {
  const { t, i18n } = useTranslation(["inventory", "common"]);
  const history = useStockMovements(product.id);
  const [reason, setReason] = useState("all");

  const rows = useMemo(() => history.data?.pages.flatMap((p) => p.rows) ?? [], [history.data]);

  // The balance after each movement: today's count, wound back through the newer movements.
  const entries = useMemo<Entry[]>(() => {
    const out: Entry[] = [];
    let newer = 0;
    for (const row of rows) {
      out.push({ row, balance: product.track_stock ? product.on_hand - newer : null });
      newer += row.qty_delta;
    }
    return out;
  }, [rows, product.on_hand, product.track_stock]);

  const reasons = useMemo(() => {
    const counts = new Map<string, number>();
    for (const r of rows) counts.set(r.reason, (counts.get(r.reason) ?? 0) + 1);
    return [...counts.entries()];
  }, [rows]);

  const shown = reason === "all" ? entries : entries.filter((e) => e.row.reason === reason);
  const days = useMemo<Day[]>(() => {
    const out: Day[] = [];
    for (const e of shown) {
      const last = out.at(-1);
      if (last && last.date === e.row.business_date) {
        last.entries.push(e);
        last.net += e.row.qty_delta;
      } else out.push({ date: e.row.business_date, entries: [e], net: e.row.qty_delta });
    }
    return out;
  }, [shown]);

  const received = rows.reduce((n, r) => (r.qty_delta > 0 ? n + r.qty_delta : n), 0);
  const removed = rows.reduce((n, r) => (r.qty_delta < 0 ? n + r.qty_delta : n), 0);

  const dayFmt = new Intl.DateTimeFormat(i18n.language, {
    weekday: "long",
    day: "numeric",
    month: "short",
    year: "numeric",
  });
  const timeFmt = new Intl.DateTimeFormat(i18n.language, { hour: "2-digit", minute: "2-digit" });
  const codes = [product.sku, product.barcode].filter((v): v is string => Boolean(v));
  const loadError =
    history.error instanceof CommandError ? translateError(i18n.t, history.error.error) : null;

  return (
    <Sheet open onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 p-0 sm:max-w-2xl">
        <SheetHeader className="border-b bg-gradient-to-b from-muted/50 to-transparent px-6 py-5">
          <div className="flex items-center gap-4 pr-8">
            <span
              aria-hidden
              className="flex size-12 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary ring-1 ring-primary/15"
            >
              <History className="size-6" />
            </span>
            <div className="min-w-0 flex-1">
              <SheetTitle className="truncate text-xl tracking-tight">{t("history.title")}</SheetTitle>
              <SheetDescription className="truncate">
                <span className="font-medium text-foreground">{product.name}</span>
                {codes.length > 0 && <span className="font-mono text-xs"> · {codes.join(" · ")}</span>}
              </SheetDescription>
            </div>
            {product.track_stock && (
              <div className="text-right">
                <div className="text-xs text-muted-foreground">{t("history.onHand")}</div>
                <div className="text-2xl font-semibold leading-none tabular-nums">{product.on_hand}</div>
              </div>
            )}
          </div>
        </SheetHeader>

        <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-6 py-5">
          <div className="grid gap-3 sm:grid-cols-3">
            <SummaryCard
              icon={<ArrowDownToLine aria-hidden />}
              tone="in"
              label={t("history.stockIn")}
              value={signed(received)}
            />
            <SummaryCard
              icon={<ArrowUpFromLine aria-hidden />}
              tone="out"
              label={t("history.stockOut")}
              value={String(removed)}
            />
            <SummaryCard
              icon={<Scale aria-hidden />}
              tone="net"
              label={t("history.net")}
              value={signed(received + removed)}
            />
          </div>
          {history.hasNextPage && (
            <p className="-mt-2 text-xs text-muted-foreground">
              {t("history.partial", { count: rows.length })}
            </p>
          )}

          {reasons.length > 1 && (
            <div role="group" aria-label={t("history.filterLabel")} className="flex flex-wrap gap-2">
              <FilterChip active={reason === "all"} onClick={() => setReason("all")}>
                {t("history.all")}
                <span className="tabular-nums opacity-70">{rows.length}</span>
              </FilterChip>
              {reasons.map(([key, count]) => (
                <FilterChip key={key} active={reason === key} onClick={() => setReason(key)}>
                  {t(`history.reasons.${key}`, { defaultValue: key })}
                  <span className="tabular-nums opacity-70">{count}</span>
                </FilterChip>
              ))}
            </div>
          )}

          {loadError && (
            <p role="alert" className="rounded-xl bg-destructive px-4 py-3 text-destructive-foreground">
              {loadError}
            </p>
          )}

          {history.isLoading ? (
            <div className="flex flex-col gap-2" aria-hidden>
              {Array.from({ length: 4 }, (_, i) => (
                <Skeleton key={i} className="h-20 w-full rounded-xl" />
              ))}
              <span className="sr-only">{t("common:table.loading")}</span>
            </div>
          ) : days.length === 0 ? (
            <div className="flex flex-col items-center gap-2 py-12 text-center text-muted-foreground">
              <span className="flex size-12 items-center justify-center rounded-full bg-muted">
                <History aria-hidden className="size-6" />
              </span>
              <p>{t("history.empty")}</p>
            </div>
          ) : (
            <ol className="flex flex-col gap-6">
              {days.map((day) => (
                <li key={day.date} className="flex flex-col gap-2">
                  <div className="flex items-center gap-3">
                    <h3 className="text-sm font-semibold">{dayFmt.format(businessDay(day.date))}</h3>
                    <span aria-hidden className="h-px flex-1 bg-border" />
                    <span className="text-xs font-medium tabular-nums text-muted-foreground">
                      {t("history.dayNet", { value: signed(day.net) })}
                    </span>
                  </div>
                  <ul className="flex flex-col gap-2">
                    {day.entries.map(({ row, balance }) => (
                      <MovementItem
                        key={row.id}
                        row={row}
                        balance={balance}
                        time={timeFmt.format(new Date(row.created_at))}
                      />
                    ))}
                  </ul>
                </li>
              ))}
            </ol>
          )}

          {history.hasNextPage && (
            <Button
              type="button"
              size="pos"
              variant="outline"
              className="self-center rounded-xl"
              disabled={history.isFetchingNextPage}
              onClick={() => void history.fetchNextPage()}
            >
              {history.isFetchingNextPage && <Loader2 aria-hidden className="animate-spin" />}
              {t("history.loadMore")}
            </Button>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}

const TONES = {
  in: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
  out: "bg-destructive/10 text-destructive",
  net: "bg-primary/10 text-primary",
} as const;

function SummaryCard({
  icon,
  tone,
  label,
  value,
}: {
  icon: ReactNode;
  tone: keyof typeof TONES;
  label: string;
  value: string;
}) {
  return (
    <Card className="flex-row items-center gap-3 px-4 py-3 shadow-sm">
      <span
        className={cn(
          "flex size-10 shrink-0 items-center justify-center rounded-lg [&_svg]:size-5",
          TONES[tone],
        )}
      >
        {icon}
      </span>
      <div className="min-w-0">
        <div className="truncate text-xs text-muted-foreground">{label}</div>
        <div className="text-xl font-semibold leading-tight tabular-nums">{value}</div>
      </div>
    </Card>
  );
}

function FilterChip({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "inline-flex min-h-10 items-center gap-2 rounded-full border px-4 text-sm font-medium transition-colors outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50",
        active
          ? "border-primary bg-primary text-primary-foreground"
          : "bg-background text-muted-foreground hover:bg-muted hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

function MovementItem({ row, balance, time }: { row: MovementRow; balance: number | null; time: string }) {
  const { t } = useTranslation("inventory");
  const Icon = REASON_ICONS[row.reason] ?? SlidersHorizontal;
  const up = row.qty_delta > 0;
  const down = row.qty_delta < 0;
  const value = Math.abs(row.qty_delta) * row.unit_cost;
  return (
    <li className="flex items-start gap-3 rounded-xl border bg-card p-3 transition-colors hover:bg-muted/40">
      <span
        aria-hidden
        className={cn(
          "flex size-10 shrink-0 items-center justify-center rounded-lg [&_svg]:size-5",
          up ? TONES.in : down ? TONES.out : "bg-muted text-muted-foreground",
        )}
      >
        <Icon />
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <div className="flex flex-wrap items-baseline gap-x-2">
          <span className="font-medium">
            {t(`history.reasons.${row.reason}`, { defaultValue: row.reason })}
          </span>
          <span className="text-xs tabular-nums text-muted-foreground">{time}</span>
        </div>
        {row.note && <p className="text-sm text-muted-foreground">{row.note}</p>}
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
          <Badge variant="outline" className="font-mono font-normal">
            {t("history.batchCode", { code: row.batch_code })}
          </Badge>
          <span className="tabular-nums">{t("history.costEach", { cost: formatMoney(row.unit_cost) })}</span>
          <span className="tabular-nums">{t("history.value", { value: formatMoney(value) })}</span>
          {row.staff_name && <span>{t("history.byName", { name: row.staff_name })}</span>}
          {row.approved_by_name && <span>{t("history.approvedBy", { name: row.approved_by_name })}</span>}
        </div>
      </div>
      <div className="text-right">
        <div
          className={cn(
            "text-lg font-semibold tabular-nums",
            up && "text-emerald-600 dark:text-emerald-400",
            down && "text-destructive",
          )}
        >
          {signed(row.qty_delta)}
        </div>
        {balance !== null && (
          <div className="text-xs tabular-nums text-muted-foreground">
            {t("history.balance", { count: balance })}
          </div>
        )}
      </div>
    </li>
  );
}
