import { QueryClient } from "@tanstack/react-query";

/** Local IPC is fast and offline-first: no automatic retries, no refetch on window focus. */
export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, refetchOnWindowFocus: false, staleTime: 30_000 },
      mutations: { retry: false },
    },
  });
}
