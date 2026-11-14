import type { Column } from "@tanstack/react-table";
import { ArrowDown, ArrowUp, ChevronsUpDown, EyeOff } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";

/** A sortable column header: click for a menu with Asc / Desc / Hide (shadcn data-table pattern). */
export function DataTableColumnHeader<TData, TValue>({
  column,
  title,
  className,
}: {
  column: Column<TData, TValue>;
  title: string;
  className?: string;
}) {
  const { t } = useTranslation("common");
  const isRight = className?.includes("justify-end") || className?.includes("text-right");

  if (!column.getCanSort()) return <div className={cn("flex items-center", className)}>{title}</div>;
  const sorted = column.getIsSorted();
  return (
    <div className={cn("flex items-center w-full", className)}>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            className={cn(
              "h-10 gap-1.5 px-3 font-semibold data-[state=open]:bg-accent",
              isRight ? "-mr-3" : "-ml-3",
            )}
          >
            <span>{title}</span>
            {sorted === "asc" ? (
              <ArrowUp aria-hidden />
            ) : sorted === "desc" ? (
              <ArrowDown aria-hidden />
            ) : (
              <ChevronsUpDown aria-hidden className="text-muted-foreground" />
            )}
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align={isRight ? "end" : "start"}>
          <DropdownMenuItem className="min-h-10" onSelect={() => column.toggleSorting(false)}>
            <ArrowUp aria-hidden />
            {t("table.sortAsc")}
          </DropdownMenuItem>
          <DropdownMenuItem className="min-h-10" onSelect={() => column.toggleSorting(true)}>
            <ArrowDown aria-hidden />
            {t("table.sortDesc")}
          </DropdownMenuItem>
          {column.getCanHide() && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem className="min-h-10" onSelect={() => column.toggleVisibility(false)}>
                <EyeOff aria-hidden />
                {t("table.hide")}
              </DropdownMenuItem>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
