import {
  createHashHistory,
  createRootRoute,
  createRoute,
  createRouter,
  redirect,
} from "@tanstack/react-router";
import { AuditLogScreen } from "@/features/audit/AuditLogScreen";
import { BatchesScreen } from "@/features/batches/BatchesScreen";
import { CashScreen } from "@/features/cash/CashScreen";
import { HomePage } from "./HomePage";
import { PosScreen } from "@/features/pos/PosScreen";
import { QuickSalesScreen } from "@/features/quick-sales/QuickSalesScreen";
import { CustomersScreen } from "@/features/customers/CustomersScreen";
import { ExpensesScreen } from "@/features/expenses/ExpensesScreen";
import { OrdersScreen } from "@/features/orders/OrdersScreen";
import { ReportsScreen } from "@/features/reports/ReportsScreen";
import { StaffScreen } from "@/features/staff/StaffScreen";
import { AppShell } from "./AppShell";
import { isProductsTab, ProductsStockPage, type ProductsTab } from "./ProductsStockPage";
import { SettingsPage } from "./SettingsPage";

// Hash history: the app is served from Tauri's asset protocol, so routes must not depend on a server.
const rootRoute = createRootRoute({ component: AppShell });

export const homeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: HomePage,
});
export const auditRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/audit",
  component: AuditLogScreen,
});

export const productsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/products",
  validateSearch: (search: Record<string, unknown>): { tab?: ProductsTab } =>
    isProductsTab(search.tab) && search.tab !== "products" ? { tab: search.tab } : {},
  component: function ProductsRoute() {
    const { tab } = productsRoute.useSearch();
    return <ProductsStockPage tab={tab} />;
  },
});

export const batchesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/batches",
  component: BatchesScreen,
});

export const cashRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/cash",
  component: CashScreen,
});

export const posRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/pos",
  component: PosScreen,
});

export const inventoryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/inventory",
  // Stock now lives on Products & Stock; old links and bookmarks land on its adjustments tab.
  beforeLoad: () => {
    throw redirect({ to: "/products", search: { tab: "adjustments" } });
  },
});

export const quickSalesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/quick-sales",
  component: QuickSalesScreen,
});

export const customersRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/customers",
  component: CustomersScreen,
});

export const expensesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/expenses",
  component: ExpensesScreen,
});

export const reportsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/reports",
  component: ReportsScreen,
});

export const ordersRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/orders",
  component: OrdersScreen,
});

export const staffRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/staff",
  component: StaffScreen,
});

export const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: SettingsPage,
});

export const routeTree = rootRoute.addChildren([
  homeRoute,
  posRoute,
  cashRoute,
  productsRoute,
  batchesRoute,
  auditRoute,
  inventoryRoute,
  quickSalesRoute,
  customersRoute,
  expensesRoute,
  reportsRoute,
  ordersRoute,
  staffRoute,
  settingsRoute,
]);

export function createAppRouter() {
  return createRouter({ routeTree, history: createHashHistory() });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
