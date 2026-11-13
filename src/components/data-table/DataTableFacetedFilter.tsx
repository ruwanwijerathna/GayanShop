import type { Column, Row } from "@tanstack/react-table";
import { PlusCircle } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

/**
 * Exact-match filter for a faceted column whose cell value is one string. TanStack's built-in
 * `arrIncludesSome` falls back to substring matching on strings ("active" would match "inactive").
 */
export function facetFilterFn<TData>(row: Row<TData>, columnId: string, value: string[]): boolean {
  return value.includes(String(row.getValue(columnId)));
}
facetFilterFn.autoRemove = (value: unknown): boolean => !Array.isArray(value) || value.length === 0;

export interface FacetOption {
  value: string;
  label: ReactNode;
}

/**
 * A multi-select filter for one column (shadcn data-table "faceted filter"). The column's filter value is
 * the array of chosen values, so the column needs `filterFn: facetFilterFn`; the per-option counts come
 * from the faceted row model and reflect the other active filters.
 */
export function DataTableFacetedFilter<TData, TValue>({
  column,
  title,
  options,
}: {
  column: Column<TData, TValue> | undefined;
  title: string;
  options: readonly FacetOption[];
}) {
  const { t } = useTranslation("common");
  const facets = column?.getFacetedUniqueValues();
  const selected = new Set((column?.getFilterValue() as string[] | undefined) ?? []);

  const toggle = (value: string, on: boolean) => {
    const next = new Set(selected);
    if (on) next.add(value);
    else next.delete(value);
    column?.setFilterValue(next.size > 0 ? [...next] : undefined);
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="outline"
          size="pos"
          className="rounded-xl border-dashed data-[state=open]:bg-accent"
          aria-label={title}
        >
          <PlusCircle aria-hidden />
          {title}
          {selected.size > 0 && (
            <>
              <span aria-hidden className="mx-1 h-5 w-px bg-border" />
              {selected.size > 2 ? (
                <Badge variant="secondary" className="rounded-md px-1.5 font-normal">
                  {t("table.filterSelected", { count: selected.size })}
                </Badge>
              ) : (
                options
                  .filter((o) => selected.has(o.value))
                  .map((o) => (
                    <Badge key={o.value} variant="secondary" className="rounded-md px-1.5 font-normal">
                      {o.label}
                    </Badge>
                  ))
              )}
            </>
          )}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="max-h-80 w-64 overflow-y-auto">
        <DropdownMenuLabel>{title}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        {options.map((o) => (
          <DropdownMenuCheckboxItem
            key={o.value}
            className="min-h-10"
            checked={selected.has(o.value)}
            onSelect={(e) => e.preventDefault()}
            onCheckedChange={(on) => toggle(o.value, on)}
          >
            <span className="flex-1 truncate">{o.label}</span>
            <span className="ml-auto pl-3 font-mono text-xs tabular-nums text-muted-foreground">
              {facets?.get(o.value) ?? 0}
            </span>
          </DropdownMenuCheckboxItem>
        ))}
        {selected.size > 0 && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              className="min-h-10 justify-center"
              onSelect={() => column?.setFilterValue(undefined)}
            >
              {t("table.clearFilter")}
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
