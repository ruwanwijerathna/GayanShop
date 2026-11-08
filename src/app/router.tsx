import { createHashHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router";
import { AuditLogScreen } from "@/features/audit/AuditLogScreen";
import { CounterStatus } from "@/features/home/CounterStatus";
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

export const routeTree = rootRoute.addChildren([homeRoute, auditRoute]);

export function createAppRouter() {
  return createRouter({ routeTree, history: createHashHistory() });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
