// Order history (doc §1.4, REPORT_SALES): every sealed order, newest first, on the virtualised DataTable
// with server-side keyset paging (ix_orders_keyset). Filter by period, counter, status or receipt number;
// open one to see its receipt; export the orders or their lines to CSV (DATA_EXPORT).
import type { ColumnDef } from "@tanstack/react-table";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { DataTable } from "@/components/data-table/DataTable";
import { ExportButton } from "@/components/export/ExportButton";
import { PeriodPicker, periodFor } from "@/components/period/PeriodPicker";
import { ReceiptDialog } from "@/components/receipt/ReceiptDialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { OrderRow_Serialize as OrderRow, OrderStatusFilter, Period } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { useBusinessToday, useOrderHistory, useOrderReceipt, useReportDevices } from "@/db/hooks";
import { translateError } from "@/i18n";
import { formatMoney } from "@/lib/money";

const FIELD = "h-12 text-base";
const SELECT = "h-12 w-full rounded-md border border-input bg-background px-3 text-base";

function Receipt({ orderId, onClose }: { orderId: string; onClose: () => void }) {
  const receipt = useOrderReceipt(orderId);
  return <ReceiptDialog receipt={receipt.data} error={receipt.error} onClose={onClose} />;
}

function Orders({ today }: { today: string }) {
  const { t, i18n } = useTranslation(["orders", "common"]);
  const [period, setPeriod] = useState<Period>(() => periodFor("day", today));
  const [deviceId, setDeviceId] = useState("");
  const [status, setStatus] = useState<OrderStatusFilter | "">("");
  const [receipt, setReceipt] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const devices = useReportDevices();
  const pages = useOrderHistory({
    period,
    device_id: deviceId || null,
    status: status || null,
    receipt: receipt.trim() || null,
  });
  const rows = useMemo(() => pages.data?.pages.flatMap((p) => p.rows) ?? [], [pages.data]);
  const columns = useMemo<ColumnDef<OrderRow>[]>(
    () => [
      {
        id: "receipt",
        header: t("columns.receipt"),
        accessorFn: (r) => r.receipt_no,
        size: 190,
        enableSorting: false,
      },
      {
        id: "when",
        header: t("columns.when"),
        accessorFn: (r) =>
          new Date(r.completed_at).toLocaleString(i18n.resolvedLanguage === "si" ? "si-LK" : "en-LK"),
        size: 200,
        enableSorting: false,
      },
      {
        id: "cashier",
        header: t("columns.cashier"),
        accessorFn: (r) => r.cashier ?? "",
        size: 140,
        enableSorting: false,
      },
      {
        id: "items",
        header: t("columns.items"),
        accessorFn: (r) => r.item_count,
        size: 80,
        enableSorting: false,
      },
      {
        id: "total",
        header: t("columns.total"),
        accessorFn: (r) => formatMoney(r.grand_total),
        size: 140,
        enableSorting: false,
      },
      {
        id: "status",
        header: t("columns.status"),
        accessorFn: (r) =>
          r.status === "voided"
            ? `${t("status.voided")}${r.void_reason ? ` · ${t(`reasons.${r.void_reason}`, { defaultValue: r.void_reason })}` : ""}`
            : r.has_customer
              ? t("status.withCustomer")
              : t("status.completed"),
        size: 220,
        enableSorting: false,
      },
    ],
    [t, i18n],
  );
  const error = pages.error instanceof CommandError ? translateError(i18n.t, pages.error.error) : null;
  return (
    <section className="flex flex-col gap-4">
      <header className="flex flex-wrap items-center gap-3">
        <h1 className="flex-1 text-2xl font-semibold">{t("title")}</h1>
        <ExportButton kind="orders" period={period} />
        <ExportButton kind="order_lines" period={period} />
      </header>
      <PeriodPicker value={period} onChange={setPeriod} today={today} />
      <div className="grid gap-3 sm:grid-cols-3">
        <div className="flex flex-col gap-2">
          <Label htmlFor="orders-receipt">{t("filters.receipt")}</Label>
          <Input
            id="orders-receipt"
            type="search"
            className={FIELD}
            value={receipt}
            onChange={(e) => setReceipt(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="orders-device">{t("filters.counter")}</Label>
          <select
            id="orders-device"
            className={SELECT}
            value={deviceId}
            onChange={(e) => setDeviceId(e.target.value)}
          >
            <option value="">{t("filters.allCounters")}</option>
            {(devices.data ?? []).map((d) => (
              <option key={d.device_id} value={d.device_id}>
                {d.device_code}
              </option>
            ))}
          </select>
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="orders-status">{t("filters.status")}</Label>
          <select
            id="orders-status"
            className={SELECT}
            value={status}
            onChange={(e) => setStatus(e.target.value === "" ? "" : (e.target.value as OrderStatusFilter))}
          >
            <option value="">{t("filters.allStatuses")}</option>
            <option value="completed">{t("status.completed")}</option>
            <option value="voided">{t("status.voided")}</option>
          </select>
        </div>
      </div>
      {error && (
        <p role="alert" className="text-base text-destructive">
          {error}
        </p>
      )}
      <DataTable<OrderRow>
        ariaLabel={t("title")}
        columns={columns}
        data={rows}
        getRowId={(r) => r.id}
        height={520}
        rowHeight={48}
        emptyLabel={pages.isLoading ? t("common:table.loading") : t("empty")}
        onRowActivate={(r) => setOpen(r.id)}
        onEndReached={() => {
          if (pages.hasNextPage && !pages.isFetchingNextPage) void pages.fetchNextPage();
        }}
      />
      {open && <Receipt orderId={open} onClose={() => setOpen(null)} />}
    </section>
  );
}

export function OrdersScreen() {
  const today = useBusinessToday();
  return today.data ? <Orders today={today.data} /> : null;
}
