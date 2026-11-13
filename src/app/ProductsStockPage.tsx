import { ArrowLeftRight, MoreVertical, Scale } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Can } from "@/components/auth/Can";
import { ExportButton } from "@/components/export/ExportButton";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { StockPanel } from "@/features/inventory/StockPanel";
import { ProductImportPanel } from "@/features/products/ProductImportPanel";
import { ProductsScreen } from "@/features/products/ProductsScreen";
import { hasPerm } from "@/lib/permissions";
import { useSession } from "./session";

type Drawer = "valuation" | "data";

/**
 * Products & Stock: the catalog with live on-hand counts. Page-wide tools sit behind the ⋮ beside the
 * title and open in a side drawer (stock valuation; export and import). Changing one product's stock
 * happens on its row (Restock / Adjust stock); adding a product is the toolbar button. Composed in
 * `src/app` because features never import each other.
 */
export function ProductsStockPage() {
  const { t } = useTranslation(["products", "app"]);
  const { session } = useSession();
  const [drawer, setDrawer] = useState<Drawer | null>(null);
  const canValue = hasPerm(session, "REPORT_FINANCIAL");
  const canData = hasPerm(session, "DATA_EXPORT") || hasPerm(session, "PRODUCT_MANAGE");

  return (
    <section className="flex flex-col gap-5">
      <header className="flex items-start gap-2">
        <div className="flex min-w-0 flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight sm:text-3xl">{t("app:nav.products")}</h1>
          <p className="text-sm text-muted-foreground">{t("subtitle")}</p>
        </div>
        {(canValue || canData) && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="icon-pos"
                className="rounded-xl data-[state=open]:bg-accent"
                aria-label={t("tools.label")}
              >
                <MoreVertical aria-hidden />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              {canValue && (
                <DropdownMenuItem className="min-h-11" onSelect={() => setDrawer("valuation")}>
                  <Scale aria-hidden />
                  {t("tools.valuation")}
                </DropdownMenuItem>
              )}
              {canData && (
                <DropdownMenuItem className="min-h-11" onSelect={() => setDrawer("data")}>
                  <ArrowLeftRight aria-hidden />
                  {t("tools.data")}
                </DropdownMenuItem>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </header>
      <ProductsScreen />
      <Sheet open={drawer !== null} onOpenChange={(o) => !o && setDrawer(null)}>
        <SheetContent className="w-full gap-0 p-0 sm:max-w-2xl">
          <SheetHeader className="border-b bg-gradient-to-b from-muted/50 to-transparent px-6 py-5">
            <SheetTitle className="text-xl tracking-tight">
              {drawer === "data" ? t("tools.data") : t("tools.valuation")}
            </SheetTitle>
            <SheetDescription>
              {drawer === "data" ? t("tools.dataHint") : t("tools.valuationHint")}
            </SheetDescription>
          </SheetHeader>
          <div className="flex-1 overflow-y-auto px-6 py-5">
            {drawer === "valuation" && (
              <StockPanel sections={["valuation", "oversold"]} label={t("tools.valuation")} />
            )}
            {drawer === "data" && (
              <div className="flex flex-col gap-6">
                <section className="flex flex-col gap-3" aria-label={t("tools.export")}>
                  <h2 className="text-lg font-semibold">{t("tools.export")}</h2>
                  <p className="text-sm text-muted-foreground">{t("tools.exportHint")}</p>
                  <div className="self-start">
                    <ExportButton kind="products" period={{ kind: "all_time" }} />
                  </div>
                </section>
                <Can perm="PRODUCT_MANAGE">
                  <section className="flex flex-col gap-3 border-t pt-6" aria-label={t("tools.import")}>
                    <h2 className="text-lg font-semibold">{t("tools.import")}</h2>
                    <ProductImportPanel />
                  </section>
                </Can>
              </div>
            )}
          </div>
        </SheetContent>
      </Sheet>
    </section>
  );
}
