// TanStack Query hooks over the generated commands. Only src/db and src/hardware call Rust (AGENT.md §2).
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  commands,
  type CategoryInput,
  type Perm,
  type ProductInput,
  type SetupInput,
  type UiLanguage,
  type UiTheme,
} from "./bindings";
import { unwrap } from "./client";

export const queryKeys = {
  status: ["app", "status"] as const,
  devicePrefs: ["app", "device-prefs"] as const,
  appInfo: ["app", "info"] as const,
  integrity: ["app", "integrity"] as const,
  loginStaff: ["auth", "login-staff"] as const,
  audit: ["audit", "pages"] as const,
  categories: ["catalog", "categories"] as const,
  products: ["catalog", "products"] as const,
  productSearch: (query: string) => ["catalog", "search", query] as const,
};

/** Polled so the shell notices an idle auto-lock done by Rust (doc §3.5). */
export function useAppStatus() {
  return useQuery({
    queryKey: queryKeys.status,
    queryFn: () => unwrap(commands.appStatus()),
    refetchInterval: 30_000,
  });
}

export function useDevicePrefs() {
  return useQuery({ queryKey: queryKeys.devicePrefs, queryFn: () => unwrap(commands.devicePreferences()) });
}

export function useAppInfo() {
  return useQuery({ queryKey: queryKeys.appInfo, queryFn: () => unwrap(commands.appInfo()) });
}

export function useIntegrity() {
  return useQuery({ queryKey: queryKeys.integrity, queryFn: () => unwrap(commands.integrityCheck()) });
}

export function useLoginStaff(enabled = true) {
  return useQuery({
    queryKey: queryKeys.loginStaff,
    queryFn: () => unwrap(commands.listLoginStaff()),
    enabled,
  });
}

const AUDIT_PAGE = 200;

export function useAuditPages() {
  return useInfiniteQuery({
    queryKey: queryKeys.audit,
    initialPageParam: null as { created_at: string; id: string } | null,
    queryFn: ({ pageParam }) => unwrap(commands.auditPage(pageParam, AUDIT_PAGE)),
    getNextPageParam: (last) => last.next_cursor ?? null,
  });
}

export function useAuditVerify() {
  return useMutation({ mutationFn: () => unwrap(commands.auditVerify()) });
}

export function useFirstRunSetup() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: SetupInput) => unwrap(commands.firstRunSetup(input)),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["app"] }),
  });
}

export function useLogin() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ staffId, pin }: { staffId: string; pin: string }) => unwrap(commands.login(staffId, pin)),
    onSuccess: () => qc.invalidateQueries({ queryKey: queryKeys.status }),
  });
}

export function useLogout() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => unwrap(commands.logout()),
    onSuccess: () => {
      qc.removeQueries({ queryKey: queryKeys.audit });
      return qc.invalidateQueries({ queryKey: queryKeys.status });
    },
  });
}

export function useRequestApproval() {
  return useMutation({
    mutationFn: (v: { perm: Perm; targetRef: string; approverId: string; pin: string }) =>
      unwrap(commands.requestApproval(v.perm, v.targetRef, v.approverId, v.pin)),
  });
}

export function useSetUiLanguage() {
  return useMutation({ mutationFn: (language: UiLanguage) => unwrap(commands.setUiLanguage(language)) });
}

export function useSetDeviceTheme() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (theme: UiTheme) => unwrap(commands.setDeviceTheme(theme)),
    onSuccess: () => qc.invalidateQueries({ queryKey: queryKeys.devicePrefs }),
  });
}

// ───────── catalog ─────────

export function useCategories() {
  return useQuery({ queryKey: queryKeys.categories, queryFn: () => unwrap(commands.listCategories()) });
}

/** Every live product, loaded once and filtered client-side (bounded list, doc §1.4). */
export function useProducts() {
  return useQuery({ queryKey: queryKeys.products, queryFn: () => unwrap(commands.listProducts()) });
}

export const SEARCH_LIMIT = 20;

/** Till type-ahead (Rust FTS). Disabled for an empty query. */
export function useProductSearch(query: string) {
  const q = query.trim();
  return useQuery({
    queryKey: queryKeys.productSearch(q),
    queryFn: () => unwrap(commands.searchProducts(q, SEARCH_LIMIT)),
    enabled: q.length > 0,
    staleTime: 5_000,
  });
}

function useCatalogMutation<TArgs, TResult>(fn: (args: TArgs) => Promise<TResult>) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: () => qc.invalidateQueries({ queryKey: ["catalog"] }),
  });
}

export function useSaveProduct() {
  return useCatalogMutation(({ id, input }: { id: string | null; input: ProductInput }) =>
    unwrap(id === null ? commands.createProduct(input) : commands.updateProduct(id, input)),
  );
}

export function useDeleteProduct() {
  return useCatalogMutation((id: string) => unwrap(commands.deleteProduct(id)));
}

export function useSaveCategory() {
  return useCatalogMutation(({ id, input }: { id: string | null; input: CategoryInput }) =>
    unwrap(id === null ? commands.createCategory(input) : commands.updateCategory(id, input)),
  );
}

export function useDeleteCategory() {
  return useCatalogMutation((id: string) => unwrap(commands.deleteCategory(id)));
}
