// Shared virtualised list (doc §1.4, §1.9): TanStack Table v8 for columns/sorting, TanStack Virtual for
// 60 fps rendering of any row count, fixed row heights, keyboard row cursor (↑/↓/Home/End, Enter opens).
import {
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  useReactTable,
  type ColumnDef,
  type OnChangeFn,
  type SortingState,
} from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

export interface DataTableProps<T> {
  columns: ColumnDef<T>[];
  data: T[];
  getRowId: (row: T) => string;
  ariaLabel: string;
  /** Viewport height in px. */
  height: number;
  /** 48 px on touch layouts (default) or 36 px compact back-office density. */
  rowHeight?: 48 | 36;
  onRowActivate?: (row: T) => void;
  /** Called when the visible window nears the end (infinite keyset paging). */
  onEndReached?: () => void;
  /** Server-side sorting (unbounded lists): pass state + handler and set `manualSorting`. */
  manualSorting?: boolean;
  sorting?: SortingState;
  onSortingChange?: OnChangeFn<SortingState>;
  emptyLabel?: string;
}

const OVERSCAN = 8;
const END_THRESHOLD = 5;

export function DataTable<T>(props: DataTableProps<T>) {
  // TanStack Table returns functions React Compiler cannot memoise safely; opt this component out.
  "use no memo";
  const { columns, data, getRowId, ariaLabel, height, rowHeight = 48, onRowActivate, onEndReached } = props;
  const { t } = useTranslation();
  const [localSorting, setLocalSorting] = useState<SortingState>([]);
  const sorting = props.sorting ?? localSorting;
  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Table v8 is not React-Compiler-safe; opted out above
  const table = useReactTable({
    data,
    columns,
    getRowId: (row) => getRowId(row),
    state: { sorting },
    onSortingChange: props.onSortingChange ?? setLocalSorting,
    manualSorting: props.manualSorting ?? false,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
  });
  const rows = table.getRowModel().rows;
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: OVERSCAN,
    initialRect: { width: 0, height },
  });
  const [active, setActive] = useState(0);
  const items = virtualizer.getVirtualItems();
  const lastVisible = items.at(-1)?.index ?? -1;

  useEffect(() => {
    if (onEndReached && rows.length > 0 && lastVisible >= rows.length - END_THRESHOLD) onEndReached();
  }, [lastVisible, rows.length, onEndReached]);

  const activate = (index: number) => {
    const row = rows[index];
    if (row && onRowActivate) onRowActivate(row.original);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (rows.length === 0) return;
    const move = (to: number) => {
      const next = Math.max(0, Math.min(rows.length - 1, to));
      setActive(next);
      virtualizer.scrollToIndex(next, { align: "auto" });
      e.preventDefault();
    };
    if (e.key === "ArrowDown") move(active + 1);
    else if (e.key === "ArrowUp") move(active - 1);
    else if (e.key === "Home") move(0);
    else if (e.key === "End") move(rows.length - 1);
    else if (e.key === "Enter") {
      activate(active);
      e.preventDefault();
    }
  };

  const onClick = (e: MouseEvent<HTMLDivElement>) => {
    const rowEl = (e.target as HTMLElement).closest<HTMLElement>("[data-row-index]");
    if (!rowEl) return;
    const index = Number.parseInt(rowEl.dataset.rowIndex ?? "", 10);
    if (Number.isInteger(index)) {
      setActive(index);
      activate(index);
    }
  };

  const headerGroup = table.getHeaderGroups()[0];

  return (
    <div
      role="grid"
      aria-label={ariaLabel}
      aria-rowcount={rows.length + 1}
      aria-activedescendant={rows[active] ? `${ariaLabel}-row-${rows[active].id}` : undefined}
      tabIndex={0}
      onKeyDown={onKeyDown}
      onClick={onClick}
      className="rounded-md border border-border bg-card text-card-foreground"
    >
      <div
        role="row"
        aria-rowindex={1}
        className="flex border-b border-border bg-muted text-sm font-semibold"
      >
        {headerGroup?.headers.map((header) => {
          const sorted = header.column.getIsSorted();
          const canSort = header.column.getCanSort();
          return (
            <div
              key={header.id}
              role="columnheader"
              aria-sort={sorted === "asc" ? "ascending" : sorted === "desc" ? "descending" : "none"}
              style={{ width: header.getSize() }}
              className="flex h-12 shrink-0 items-center px-3"
            >
              {canSort ? (
                <button
                  type="button"
                  className="inline-flex h-12 items-center gap-1"
                  onClick={(e) => {
                    e.stopPropagation();
                    header.column.toggleSorting();
                  }}
                >
                  {flexRender(header.column.columnDef.header, header.getContext())}
                  {sorted === "asc" && <ArrowUp aria-hidden className="size-4" />}
                  {sorted === "desc" && <ArrowDown aria-hidden className="size-4" />}
                </button>
              ) : (
                flexRender(header.column.columnDef.header, header.getContext())
              )}
            </div>
          );
        })}
      </div>
      <div ref={scrollRef} style={{ height }} className="overflow-auto">
        {rows.length === 0 ? (
          <div className="flex h-full items-center justify-center text-muted-foreground">
            {props.emptyLabel ?? t("table.empty")}
          </div>
        ) : (
          <div style={{ height: virtualizer.getTotalSize() }} className="relative w-full">
            {items.map((item) => {
              const row = rows[item.index];
              if (!row) return null;
              return (
                <div
                  key={row.id}
                  id={`${ariaLabel}-row-${row.id}`}
                  role="row"
                  aria-rowindex={item.index + 2}
                  aria-selected={item.index === active}
                  data-row-index={item.index}
                  style={{ height: rowHeight, transform: `translateY(${item.start}px)` }}
                  className={cn(
                    "absolute top-0 left-0 flex w-full items-center border-b border-border tabular",
                    item.index % 2 === 1 && "bg-muted/40",
                    item.index === active && "bg-accent outline-2 -outline-offset-2 outline-ring",
                    onRowActivate && "cursor-pointer",
                  )}
                >
                  {row.getVisibleCells().map((cell) => (
                    <div
                      key={cell.id}
                      role="gridcell"
                      style={{ width: cell.column.getSize() }}
                      className="shrink-0 truncate px-3"
                    >
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </div>
                  ))}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
