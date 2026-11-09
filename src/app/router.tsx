import { createHashHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router";
import { AuditLogScreen } from "@/features/audit/AuditLogScreen";
import { BatchesScreen } from "@/features/batches/BatchesScreen";
import { CashScreen } from "@/features/cash/CashScreen";
import { CounterStatus } from "@/features/home/CounterStatus";
import { PosScreen } from "@/features/pos/PosScreen";
import { PrinterSettings } from "@/features/printing/PrinterSettings";
import { ProductsScreen } from "@/features/products/ProductsScreen";
import { AppShell } from "./AppShell";

// Hash history: the app is served from Tauri's asset protocol, so routes must not depend on a server.
const rootRoute = createRootRoute({ component: AppShell });

export const homeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: CounterStatus,
});
export const auditRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/audit",
  component: AuditLogScreen,
});

export const productsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/products",
  component: ProductsScreen,
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

export const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: PrinterSettings,
});

export const routeTree = rootRoute.addChildren([
  homeRoute,
  posRoute,
  cashRoute,
  productsRoute,
  batchesRoute,
  auditRoute,
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
