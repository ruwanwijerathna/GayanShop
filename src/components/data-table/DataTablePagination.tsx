import type { Table } from "@tanstack/react-table";
import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight } from "lucide-react";
import { useTranslation } from "react-i18next";
import { SelectField } from "@/components/common/SelectField";
import { Button } from "@/components/ui/button";

export const PAGE_SIZES = [10, 25, 50, 100] as const;

/** Footer: selection summary, rows-per-page and first / previous / next / last page. */
export function DataTablePagination<TData>({ table }: { table: Table<TData> }) {
  const { t } = useTranslation("common");
  const { pageIndex, pageSize } = table.getState().pagination;
  const pages = Math.max(1, table.getPageCount());
  const selected = table.getFilteredSelectedRowModel().rows.length;
  const total = table.getFilteredRowModel().rows.length;

  return (
    <div className="flex flex-col gap-3 border-t bg-muted/20 px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
      <p className="text-sm text-muted-foreground tabular-nums">
        {t("table.selectedOf", { selected, total })}
      </p>
      <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
        <div className="flex items-center gap-2 text-sm">
          <span id="rows-per-page" className="text-muted-foreground">
            {t("table.rowsPerPage")}
          </span>
          <SelectField
            aria-label={t("table.rowsPerPage")}
            className="w-24"
            value={String(pageSize)}
            onValueChange={(v) => table.setPageSize(PAGE_SIZES.find((n) => String(n) === v) ?? PAGE_SIZES[1])}
            options={PAGE_SIZES.map((n) => ({ value: String(n), label: String(n) }))}
          />
        </div>
        <span className="text-sm font-medium tabular-nums" aria-live="polite">
          {t("table.pageOf", { page: pageIndex + 1, pages })}
        </span>
        <div className="flex items-center gap-1">
          <Button
            type="button"
            variant="outline"
            size="icon-pos"
            className="hidden rounded-xl sm:inline-flex"
            aria-label={t("table.firstPage")}
            disabled={!table.getCanPreviousPage()}
            onClick={() => table.setPageIndex(0)}
          >
            <ChevronsLeft aria-hidden />
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon-pos"
            className="rounded-xl"
            aria-label={t("table.prevPage")}
            disabled={!table.getCanPreviousPage()}
            onClick={() => table.previousPage()}
          >
            <ChevronLeft aria-hidden />
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon-pos"
            className="rounded-xl"
            aria-label={t("table.nextPage")}
            disabled={!table.getCanNextPage()}
            onClick={() => table.nextPage()}
          >
            <ChevronRight aria-hidden />
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon-pos"
            className="hidden rounded-xl sm:inline-flex"
            aria-label={t("table.lastPage")}
            disabled={!table.getCanNextPage()}
            onClick={() => table.setPageIndex(table.getPageCount() - 1)}
          >
            <ChevronsRight aria-hidden />
          </Button>
        </div>
      </div>
    </div>
  );
}
