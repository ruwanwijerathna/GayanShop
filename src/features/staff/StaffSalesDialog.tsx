import { CalendarDays, CalendarRange, CalendarClock } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { StaffDailySalesBar } from "@/components/charts/StaffDailySalesBar";
import { Card } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import type { StaffMember_Serialize as StaffMember } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useStaffSales } from "@/db/hooks";
import { translateError } from "@/i18n";
import { formatMoney } from "@/lib/money";

function TotalCard({
  label,
  total,
  orders,
  icon,
}: {
  label: string;
  total: number;
  orders: number;
  icon: ReactNode;
}) {
  const { t } = useTranslation("staff");
  return (
    <Card className="flex-row items-center gap-4 px-5 py-4 shadow-sm" role="group" aria-label={label}>
      <span className="flex size-12 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary [&_svg]:size-6">
        {icon}
      </span>
      <div className="min-w-0">
        <div className="truncate text-sm text-muted-foreground">{label}</div>
        <div className="text-2xl font-semibold leading-tight tabular-nums">{formatMoney(total)}</div>
        <div className="text-xs text-muted-foreground">{t("sales.orders", { count: orders })}</div>
      </div>
    </Card>
  );
}

/** A person's daily sales chart and their week, month and year totals (opened from the row's ⋮ menu). */
export function StaffSalesDialog({ member, onClose }: { member: StaffMember; onClose: () => void }) {
  const { t, i18n } = useTranslation("staff");
  const sales = useStaffSales(member.id);
  const error = sales.error instanceof CommandError ? translateError(i18n.t, sales.error.error) : null;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-4xl">
        <DialogHeader>
          <DialogTitle className="text-xl">{t("sales.title", { name: member.display_name })}</DialogTitle>
          <DialogDescription>
            {member.staff_no === null
              ? t("sales.subtitleNoNumber")
              : t("sales.subtitle", { no: member.staff_no })}
          </DialogDescription>
        </DialogHeader>
        {error && (
          <p role="alert" className="text-base text-destructive">
            {error}
          </p>
        )}
        {sales.isLoading && <Skeleton className="h-64 w-full" />}
        {sales.data && (
          <div className="flex flex-col gap-5">
            <div className="grid gap-4 sm:grid-cols-3">
              <TotalCard
                label={t("sales.week")}
                total={sales.data.week.total}
                orders={sales.data.week.orders}
                icon={<CalendarDays aria-hidden />}
              />
              <TotalCard
                label={t("sales.month")}
                total={sales.data.month.total}
                orders={sales.data.month.orders}
                icon={<CalendarRange aria-hidden />}
              />
              <TotalCard
                label={t("sales.year")}
                total={sales.data.year.total}
                orders={sales.data.year.orders}
                icon={<CalendarClock aria-hidden />}
              />
            </div>
            <StaffDailySalesBar days={sales.data.daily} />
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
