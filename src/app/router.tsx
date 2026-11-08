import {
  createHashHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
} from "@tanstack/react-router";

// Hash history: the app is served from Tauri's asset protocol, so routes must not depend on a server.
const rootRoute = createRootRoute({ component: Outlet });

export const tillRoute = createRoute({ getParentRoute: () => rootRoute, path: "/" });
export const backOfficeRoute = createRoute({ getParentRoute: () => rootRoute, path: "/back-office" });

export const routeTree = rootRoute.addChildren([tillRoute, backOfficeRoute]);

export function createAppRouter() {
  return createRouter({ routeTree, history: createHashHistory() });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
