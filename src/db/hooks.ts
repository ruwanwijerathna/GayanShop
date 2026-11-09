// TanStack Query hooks over the generated commands. Only src/db and src/hardware call Rust (AGENT.md §2).
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  commands,
  type AdHocPurchase,
  type BatchInput,
  type BatchItemInput,
  type CartInput_Deserialize as CartInput,
  type CategoryInput,
  type CheckoutInput_Deserialize as CheckoutInput,
  type CloseInput,
  type DrawerMovementInput,
  type Perm,
  type NewStaff_Deserialize as NewStaff,
  type ProductInput,
  type ReceiptPrinterProfile,
  type SetupInput,
  type StaffInput_Deserialize as StaffInput,
  type UiLanguage,
  type UiTheme,
  type VoidInput_Deserialize as VoidInput,
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
  batches: ["batches", "list"] as const,
  batch: (id: string) => ["batches", "detail", id] as const,
  cashSession: ["cash", "current"] as const,
  printerStatus: ["printer", "status"] as const,
  printerProfile: ["printer", "profile"] as const,
  customer: (phone: string) => ["customers", "lookup", phone] as const,
  sessionOrders: ["orders", "session"] as const,
  heldCarts: ["pos", "held"] as const,
  currentCart: ["pos", "current"] as const,
  backupStatus: ["backup", "status"] as const,
  staff: ["staff", "list"] as const,
  roles: ["staff", "roles"] as const,
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

// ───────── batches (BATCH_MANAGE) ─────────

export function useBatches() {
  return useQuery({ queryKey: queryKeys.batches, queryFn: () => unwrap(commands.listBatches()) });
}

export function useBatch(id: string | null) {
  return useQuery({
    queryKey: queryKeys.batch(id ?? ""),
    queryFn: () => unwrap(commands.getBatch(id ?? "")),
    enabled: id !== null,
  });
}

function useBatchMutation<TArgs, TResult>(fn: (args: TArgs) => Promise<TResult>) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["batches"] });
      await qc.invalidateQueries({ queryKey: ["catalog"] });
    },
  });
}

export function useCreateBatch() {
  return useBatchMutation((input: BatchInput) => unwrap(commands.createBatch(input)));
}

export function useSetBatchItem() {
  return useBatchMutation(({ batchId, input }: { batchId: string; input: BatchItemInput }) =>
    unwrap(commands.setBatchItem(batchId, input)),
  );
}

export function useRemoveBatchItem() {
  return useBatchMutation(({ batchId, itemId }: { batchId: string; itemId: string }) =>
    unwrap(commands.removeBatchItem(batchId, itemId)),
  );
}

export function useReceiveBatch() {
  return useBatchMutation((id: string) => unwrap(commands.receiveBatch(id)));
}

export function useAdHocPurchase() {
  return useBatchMutation((input: AdHocPurchase) => unwrap(commands.adHocPurchase(input)));
}

// ───────── cash sessions ─────────

export function useCurrentCashSession() {
  return useQuery({ queryKey: queryKeys.cashSession, queryFn: () => unwrap(commands.currentCashSession()) });
}

function useCashMutation<TArgs, TResult>(fn: (args: TArgs) => Promise<TResult>) {
  const qc = useQueryClient();
  return useMutation({ mutationFn: fn, onSuccess: () => qc.invalidateQueries({ queryKey: ["cash"] }) });
}

export function useOpenCashSession() {
  return useCashMutation((openingFloat: number) =>
    unwrap(commands.openCashSession({ opening_float: openingFloat })),
  );
}

export function useDrawerMovement() {
  return useCashMutation(({ input, approvalId }: { input: DrawerMovementInput; approvalId: string | null }) =>
    unwrap(commands.cashDrawerMovement(input, approvalId)),
  );
}

export function useNoSale() {
  return useCashMutation(({ reason, approvalId }: { reason: string; approvalId: string | null }) =>
    unwrap(commands.noSale(reason, approvalId)),
  );
}

export function useCloseCashSession() {
  return useCashMutation((input: CloseInput) => unwrap(commands.closeCashSession(input)));
}

// ───────── receipt printer (doc §7.5) ─────────

/** Banner state. Polled as well, because the worker's health changes without a database write. */
export function usePrinterStatus() {
  return useQuery({
    queryKey: queryKeys.printerStatus,
    queryFn: () => unwrap(commands.printerStatus()),
    refetchInterval: 5_000,
  });
}

export function usePrinterProfile() {
  return useQuery({ queryKey: queryKeys.printerProfile, queryFn: () => unwrap(commands.printerProfile()) });
}

function usePrinterMutation<TArgs, TResult>(fn: (args: TArgs) => Promise<TResult>) {
  const qc = useQueryClient();
  return useMutation({ mutationFn: fn, onSuccess: () => qc.invalidateQueries({ queryKey: ["printer"] }) });
}

export function useSetPrinterProfile() {
  return usePrinterMutation((profile: ReceiptPrinterProfile) => unwrap(commands.setPrinterProfile(profile)));
}

export function usePrintTestPage() {
  return usePrinterMutation(() => unwrap(commands.printTestPage()));
}

export function usePrintRetry() {
  return usePrinterMutation(() => unwrap(commands.printRetry()));
}

export function usePrintDismissFailed() {
  return usePrinterMutation(() => unwrap(commands.printDismissFailed()));
}

export function usePrintRecoveryResolve() {
  return usePrinterMutation((printNow: boolean) => unwrap(commands.printRecoveryResolve(printNow)));
}

// ───────── till (doc §1.9) ─────────

/** The cart header after 9–10 digits (a local primary-key read, so it works offline). */
export function useCustomerLookup(phone: string | null) {
  return useQuery({
    queryKey: queryKeys.customer(phone ?? ""),
    queryFn: () => unwrap(commands.customerLookup({ phone: phone ?? "" })),
    enabled: phone !== null,
    staleTime: 30_000,
    retry: false,
  });
}

export function useCheckoutOrder() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: CheckoutInput) => unwrap(commands.checkoutOrder(input)),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["orders"] });
      await qc.invalidateQueries({ queryKey: ["cash"] });
      await qc.invalidateQueries({ queryKey: ["customers"] });
    },
  });
}

export function useSessionOrders(enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.sessionOrders,
    queryFn: () => unwrap(commands.sessionOrders()),
    enabled,
  });
}

export function useVoidOrder() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: VoidInput) => unwrap(commands.voidOrder(input)),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["orders"] });
      await qc.invalidateQueries({ queryKey: ["cash"] });
    },
  });
}

/** The crash-recovery cart: written by the till with a debounce, read once on start. */
export function saveCurrentCart(input: CartInput) {
  return unwrap(commands.cartSaveCurrent(input));
}

export function discardCurrentCart() {
  return unwrap(commands.cartDiscardCurrent());
}

export function useCurrentCart() {
  return useQuery({
    queryKey: queryKeys.currentCart,
    queryFn: () => unwrap(commands.cartCurrent()),
    staleTime: Infinity,
  });
}

export function useHeldCarts(enabled: boolean) {
  return useQuery({ queryKey: queryKeys.heldCarts, queryFn: () => unwrap(commands.heldCarts()), enabled });
}

export function useHoldCart() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: CartInput) => unwrap(commands.cartHold(input)),
    onSuccess: () => qc.invalidateQueries({ queryKey: queryKeys.heldCarts }),
  });
}

export function useRecallCart() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => unwrap(commands.cartRecall(id)),
    onSuccess: () => qc.invalidateQueries({ queryKey: queryKeys.heldCarts }),
  });
}

/** A typed barcode or SKU (Enter in the scan field). */
export function lookupProductCode(code: string) {
  return unwrap(commands.lookupProductCode(code));
}

// ───────── backup (doc §3.9) ─────────

export function useBackupStatus() {
  return useQuery({ queryKey: queryKeys.backupStatus, queryFn: () => unwrap(commands.backupStatus()) });
}

/** The native folder picker; `null` when cancelled. */
export function pickBackupFolder() {
  return unwrap(commands.backupPickFolder());
}

export function useBackupNow() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (destDir: string) => unwrap(commands.backupNow(destDir)),
    onSettled: () => qc.invalidateQueries({ queryKey: ["backup"] }),
  });
}

// ───────── staff (USER_MANAGE, doc §3.5) ─────────

export function useStaffList() {
  return useQuery({ queryKey: queryKeys.staff, queryFn: () => unwrap(commands.listStaff()) });
}

export function useRoles() {
  return useQuery({ queryKey: queryKeys.roles, queryFn: () => unwrap(commands.listRoles()) });
}

function useStaffMutation<TArgs, TResult>(fn: (args: TArgs) => Promise<TResult>) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["staff"] });
      await qc.invalidateQueries({ queryKey: queryKeys.loginStaff });
    },
  });
}

export function useCreateStaff() {
  return useStaffMutation((input: NewStaff) => unwrap(commands.createStaff(input)));
}

export function useUpdateStaff() {
  return useStaffMutation(({ id, input }: { id: string; input: StaffInput }) =>
    unwrap(commands.updateStaff(id, input)),
  );
}

export function useResetStaffPin() {
  return useStaffMutation(({ id, pin }: { id: string; pin: string }) =>
    unwrap(commands.resetStaffPin(id, pin)),
  );
}
