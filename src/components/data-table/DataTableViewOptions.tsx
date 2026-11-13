import type { Table } from "@tanstack/react-table";
import { SlidersHorizontal } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

/** The "View" menu: show or hide the columns that allow it. `labels` maps column id → display name. */
export function DataTableViewOptions<TData>({
  table,
  labels,
}: {
  table: Table<TData>;
  labels: Record<string, string>;
}) {
  const { t } = useTranslation("common");
  const columns = table.getAllColumns().filter((c) => c.getCanHide());
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button type="button" variant="outline" size="pos" className="rounded-xl">
          <SlidersHorizontal aria-hidden />
          {t("table.view")}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-48">
        <DropdownMenuLabel>{t("table.toggleColumns")}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        {columns.map((c) => (
          <DropdownMenuCheckboxItem
            key={c.id}
            className="min-h-10"
            checked={c.getIsVisible()}
            onSelect={(e) => e.preventDefault()}
            onCheckedChange={(on) => c.toggleVisibility(on)}
          >
            {labels[c.id] ?? c.id}
          </DropdownMenuCheckboxItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
