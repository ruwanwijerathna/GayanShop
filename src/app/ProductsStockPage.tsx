import { useNavigate } from "@tanstack/react-router";
import { ArrowLeftRight, History, Package, Scale, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { PosTabsList, PosTabsTrigger } from "@/components/common/PosTabs";
import { Tabs, TabsContent } from "@/components/ui/tabs";
import type { Perm } from "@/db/bindings";
import { STOCK_SECTION_PERMS, StockPanel, type StockSection } from "@/features/inventory/StockPanel";
import { ProductsScreen } from "@/features/products/ProductsScreen";
import { hasPerm } from "@/lib/permissions";
import { useSession } from "./session";

export const PRODUCTS_TABS = ["products", "adjustments", "valuation", "history"] as const;
export type ProductsTab = (typeof PRODUCTS_TABS)[number];

export function isProductsTab(value: unknown): value is ProductsTab {
  return typeof value === "string" && (PRODUCTS_TABS as readonly string[]).includes(value);
}

/** Which stock sections each tab holds (the catalog tab holds none). */
const SECTIONS: Record<Exclude<ProductsTab, "products">, readonly StockSection[]> = {
  adjustments: ["count", "damage"],
  valuation: ["valuation", "oversold"],
  history: ["history"],
};

const ICONS: Record<ProductsTab, LucideIcon> = {
  products: Package,
  adjustments: ArrowLeftRight,
  valuation: Scale,
  history: History,
};

/** A tab shows when the staff member may open at least one of its sections. */
function tabPerms(tab: ProductsTab): Perm[] {
  return tab === "products" ? [] : SECTIONS[tab].map((s) => STOCK_SECTION_PERMS[s]);
}

/**
 * Products & Stock: the catalog with live on-hand counts, plus the stock tools (adjustments, valuation
 * and oversold layers, movement history) as tabs. The tab lives in the URL (`/products?tab=…`), so
 * `/inventory` can redirect here. Composed in `src/app` because features never import each other.
 */
export function ProductsStockPage({ tab }: { tab: ProductsTab | undefined }) {
  const { t } = useTranslation(["products", "app"]);
  const { session } = useSession();
  const navigate = useNavigate();
  const allowed = (x: ProductsTab) => {
    const perms = tabPerms(x);
    return perms.length === 0 || perms.some((p) => hasPerm(session, p));
  };
  const tabs = PRODUCTS_TABS.filter(allowed);
  const current: ProductsTab = tab && allowed(tab) ? tab : "products";

  const choose = (v: string) => {
    if (!isProductsTab(v)) return;
    void navigate({ to: "/products", search: { tab: v === "products" ? undefined : v } });
  };

  return (
    <section className="flex flex-col gap-5">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-semibold tracking-tight sm:text-3xl">{t("app:nav.products")}</h1>
        <p className="text-sm text-muted-foreground">{t("subtitle")}</p>
      </header>
      <Tabs value={current} onValueChange={choose} className="gap-5">
        <PosTabsList aria-label={t("tabs.label")}>
          {tabs.map((x) => {
            const Icon = ICONS[x];
            return (
              <PosTabsTrigger key={x} value={x}>
                <Icon aria-hidden />
                {t(`tabs.${x}`)}
              </PosTabsTrigger>
            );
          })}
        </PosTabsList>
        {tabs.map((x) => (
          <TabsContent key={x} value={x} className="motion-safe:animate-in motion-safe:fade-in-0">
            {x === "products" ? (
              <ProductsScreen />
            ) : (
              <StockPanel sections={SECTIONS[x]} label={t(`tabs.${x}`)} />
            )}
          </TabsContent>
        ))}
      </Tabs>
    </section>
  );
}
