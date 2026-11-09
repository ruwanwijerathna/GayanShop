// Report period picker (doc §5.2): day, week, month, year, all time or a custom inclusive range. Rust
// resolves the period against the store calendar (week start, business dates), so this only builds it.
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { Period } from "@/db/bindings";

export type PeriodKind = Period["kind"];
export const PERIOD_KINDS: PeriodKind[] = ["day", "week", "month", "year", "all_time", "custom"];

const FIELD = "h-12 text-base";

function yearMonth(date: string): { year: number; month: number } {
  return { year: Number.parseInt(date.slice(0, 4), 10), month: Number.parseInt(date.slice(5, 7), 10) };
}

/** The default period of a kind, anchored on today's business date (`YYYY-MM-DD`). */
export function periodFor(kind: PeriodKind, today: string): Period {
  switch (kind) {
    case "day":
      return { kind, date: today };
    case "week":
      return { kind, any_date: today };
    case "month":
      return { kind, ...yearMonth(today) };
    case "year":
      return { kind, year: yearMonth(today).year };
    case "all_time":
      return { kind };
    case "custom":
      return { kind, from: `${today.slice(0, 8)}01`, to: today };
  }
}

const DATE = /^\d{4}-\d{2}-\d{2}$/;

export function PeriodPicker({
  value,
  onChange,
  today,
}: {
  value: Period;
  onChange: (p: Period) => void;
  today: string;
}) {
  const { t } = useTranslation();
  const setDate = (text: string, make: (d: string) => Period) => {
    if (DATE.test(text)) onChange(make(text));
  };
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap gap-2" role="group" aria-label={t("period.label")}>
        {PERIOD_KINDS.map((k) => (
          <Button
            key={k}
            type="button"
            size="pos"
            variant={value.kind === k ? "default" : "outline"}
            aria-pressed={value.kind === k}
            onClick={() => onChange(periodFor(k, today))}
          >
            {t(`period.${k}`)}
          </Button>
        ))}
      </div>
      <div className="flex flex-wrap items-end gap-3">
        {value.kind === "day" && (
          <div className="flex flex-col gap-2">
            <Label htmlFor="period-day">{t("period.date")}</Label>
            <Input
              id="period-day"
              type="date"
              className={FIELD}
              value={value.date}
              onChange={(e) => setDate(e.target.value, (date) => ({ kind: "day", date }))}
            />
          </div>
        )}
        {value.kind === "week" && (
          <div className="flex flex-col gap-2">
            <Label htmlFor="period-week">{t("period.weekOf")}</Label>
            <Input
              id="period-week"
              type="date"
              className={FIELD}
              value={value.any_date}
              onChange={(e) => setDate(e.target.value, (any_date) => ({ kind: "week", any_date }))}
            />
          </div>
        )}
        {value.kind === "month" && (
          <div className="flex flex-col gap-2">
            <Label htmlFor="period-month">{t("period.month")}</Label>
            <Input
              id="period-month"
              type="month"
              className={FIELD}
              value={`${value.year}-${String(value.month).padStart(2, "0")}`}
              onChange={(e) => {
                if (/^\d{4}-\d{2}$/.test(e.target.value))
                  onChange({ kind: "month", ...yearMonth(`${e.target.value}-01`) });
              }}
            />
          </div>
        )}
        {value.kind === "year" && (
          <div className="flex flex-col gap-2">
            <Label htmlFor="period-year">{t("period.year")}</Label>
            <Input
              id="period-year"
              inputMode="numeric"
              className={FIELD}
              value={String(value.year)}
              onChange={(e) => {
                if (/^\d{4}$/.test(e.target.value))
                  onChange({ kind: "year", year: Number.parseInt(e.target.value, 10) });
              }}
            />
          </div>
        )}
        {value.kind === "custom" && (
          <>
            <div className="flex flex-col gap-2">
              <Label htmlFor="period-from">{t("period.from")}</Label>
              <Input
                id="period-from"
                type="date"
                className={FIELD}
                value={value.from}
                onChange={(e) => setDate(e.target.value, (from) => ({ ...value, from }))}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="period-to">{t("period.to")}</Label>
              <Input
                id="period-to"
                type="date"
                className={FIELD}
                value={value.to}
                onChange={(e) => setDate(e.target.value, (to) => ({ ...value, to }))}
              />
            </div>
          </>
        )}
      </div>
    </div>
  );
}
