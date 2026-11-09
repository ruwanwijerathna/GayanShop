// TanStack Query hooks over the generated commands. Only src/db and src/hardware call Rust (AGENT.md §2).
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  commands,
  type AdHocPurchase,
  type BatchInput,
  type BatchItemInput,
  type CartInput_Deserialize as CartInput,
  type AdjustmentInput,
  type CategoryInput,
  type CountInput,
  type DamageInput,
  type LabelJob,
  type LabelPrinterProfile,
  type LateItemInput,
  type CheckoutInput_Deserialize as CheckoutInput,
  type CloseInput,
  type CustomerSort,
  type CustomerUpdate,
  type ExpenseCategoryInput,
  type ExpenseInput,
  type Granularity,
  type HistoryCursor,
  type Period,
  type DrawerMovementInput,
  type Perm,
  type NewStaff_Deserialize as NewStaff,
  type ProductInput,
  type PromoteInput,
  type QuickSaleStatus,
  type ReceiptPrinterProfile,
  type SetupInput,
  type StaffInput_Deserialize as StaffInput,
  type TemplateCalibration,
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
  batchPreview: (id: string) => ["batches", "preview", id] as const,
  cashSession: ["cash", "current"] as const,
  printerStatus: ["printer", "status"] as const,
  printerProfile: ["printer", "profile"] as const,
  customer: (phone: string) => ["customers", "lookup", phone] as const,
  customerList: (search: string, sort: CustomerSort) => ["customers", "list", search, sort] as const,
  customerProfile: (id: string) => ["customers", "profile", id] as const,
  customerHistory: (id: string) => ["customers", "history", id] as const,
  customerReceipt: (customerId: string, orderId: string) =>
    ["customers", "receipt", customerId, orderId] as const,
  customerAnalytics: (period: Period, g: Granularity) =>
    ["customers", "analytics", JSON.stringify(period), g] as const,
  businessToday: ["app", "today"] as const,
  expenseCategories: ["expenses", "categories"] as const,
  expenseList: (period: Period, categoryId: string | null) =>
    ["expenses", "list", JSON.stringify(period), categoryId ?? "all"] as const,
  sessionOrders: ["orders", "session"] as const,
  heldCarts: ["pos", "held"] as const,
  currentCart: ["pos", "current"] as const,
  backupStatus: ["backup", "status"] as const,
  staff: ["staff", "list"] as const,
  labelTemplates: ["labels", "templates"] as const,
  valuation: (asOf: string | null) => ["inventory", "valuation", asOf ?? "now"] as const,
  oversold: ["inventory", "oversold"] as const,
  quickSales: (show: QuickSaleStatus | null) => ["quickSales", "review", show ?? "all"] as const,
  quickSaleDetail: (key: string) => ["quickSales", "detail", key] as const,
  quickSaleHint: (name: string) => ["quickSales", "hint", name] as const,
  movements: (productId: string) => ["inventory", "movements", productId] as const,
  labelProfile: ["labels", "profile"] as const,
  labelPreview: (job: LabelJob) => ["labels", "preview", JSON.stringify(job)] as const,
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

export function useUpdateBatch() {
  return useBatchMutation(({ id, input }: { id: string; input: BatchInput }) =>
    unwrap(commands.updateBatch(id, input)),
  );
}

/** What "Receive" would fix right now (refreshed with every batch change). */
export function useBatchAllocationPreview(id: string | null) {
  return useQuery({
    queryKey: queryKeys.batchPreview(id ?? ""),
    queryFn: () => unwrap(commands.batchAllocationPreview(id ?? "")),
    enabled: id !== null,
  });
}

export function useReopenBatch() {
  return useBatchMutation((id: string) => unwrap(commands.reopenBatch(id)));
}

export function useAddLateBatchItem() {
  return useBatchMutation(({ batchId, input }: { batchId: string; input: LateItemInput }) =>
    unwrap(commands.addLateBatchItem(batchId, input)),
  );
}

export function useCloseBatch() {
  return useBatchMutation((id: string) => unwrap(commands.closeBatch(id)));
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

// ───────── barcode labels (doc §7.4) ─────────

export function useLabelTemplates() {
  return useQuery({ queryKey: queryKeys.labelTemplates, queryFn: () => unwrap(commands.labelTemplates()) });
}

export function useLabelPrinterProfile() {
  return useQuery({
    queryKey: queryKeys.labelProfile,
    queryFn: () => unwrap(commands.labelPrinterProfile()),
  });
}

function useLabelMutation<TArgs, TResult>(fn: (args: TArgs) => Promise<TResult>) {
  const qc = useQueryClient();
  return useMutation({ mutationFn: fn, onSuccess: () => qc.invalidateQueries({ queryKey: ["labels"] }) });
}

export function useSetLabelPrinterProfile() {
  return useLabelMutation((profile: LabelPrinterProfile) => unwrap(commands.setLabelPrinterProfile(profile)));
}

export function useCalibrateLabelTemplate() {
  return useLabelMutation(({ id, calibration }: { id: string; calibration: TemplateCalibration | null }) =>
    unwrap(commands.calibrateLabelTemplate(id, calibration)),
  );
}

export function useLabelAlignmentTest() {
  return useLabelMutation((templateId: string) => unwrap(commands.labelAlignmentTest(templateId)));
}

/** The Rust-computed layout of a job (what will print), for the SVG preview. */
export function useLabelPreview(job: LabelJob | null) {
  return useQuery({
    queryKey: queryKeys.labelPreview(job ?? { template_id: "", items: [], fill_last_row: false }),
    queryFn: () => unwrap(commands.labelPreview(job ?? { template_id: "", items: [], fill_last_row: false })),
    enabled: job !== null && job.items.length > 0,
    retry: false,
  });
}

export function usePrintLabels() {
  return useLabelMutation(({ job, approvalId }: { job: LabelJob; approvalId: string | null }) =>
    unwrap(commands.printLabels(job, approvalId)),
  );
}

// ───────── stock operations (doc §3.1, §5) ─────────

export function useStockValuation(asOf: string | null) {
  return useQuery({
    queryKey: queryKeys.valuation(asOf),
    queryFn: () => unwrap(commands.stockValuation(asOf)),
  });
}

export function useOversoldLayers() {
  return useQuery({ queryKey: queryKeys.oversold, queryFn: () => unwrap(commands.oversoldLayers()) });
}

const MOVEMENTS_PAGE = 100;

export function useStockMovements(productId: string | null) {
  return useInfiniteQuery({
    queryKey: queryKeys.movements(productId ?? ""),
    initialPageParam: null as { created_at: string; id: string } | null,
    queryFn: ({ pageParam }) =>
      unwrap(commands.stockMovements(productId ?? "", null, pageParam, MOVEMENTS_PAGE)),
    getNextPageParam: (last) =>
      last.next_cursor ? { created_at: last.next_cursor[0], id: last.next_cursor[1] } : null,
    enabled: productId !== null,
  });
}

function useStockMutation<TArgs, TResult>(fn: (args: TArgs) => Promise<TResult>) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["inventory"] });
      await qc.invalidateQueries({ queryKey: ["batches"] });
    },
  });
}

export function useRecordDamage() {
  return useStockMutation(({ input, approvalId }: { input: DamageInput; approvalId: string | null }) =>
    unwrap(commands.recordDamage(input, approvalId)),
  );
}

export function useAdjustStock() {
  return useStockMutation(({ input, approvalId }: { input: AdjustmentInput; approvalId: string | null }) =>
    unwrap(commands.adjustStock(input, approvalId)),
  );
}

export function useRecordStockCount() {
  return useStockMutation(({ input, approvalId }: { input: CountInput; approvalId: string | null }) =>
    unwrap(commands.recordStockCount(input, approvalId)),
  );
}

export function useWriteOffBatch() {
  return useStockMutation(({ batchId, note }: { batchId: string; note: string }) =>
    unwrap(commands.writeOffBatch(batchId, note)),
  );
}

// ───────── quick-sale review (doc §3.2) ─────────

export function useQuickSaleReview(show: QuickSaleStatus | null) {
  return useQuery({
    queryKey: queryKeys.quickSales(show),
    queryFn: () => unwrap(commands.quickSaleReview(show)),
  });
}

export function useQuickSaleDetail(key: string | null) {
  return useQuery({
    queryKey: queryKeys.quickSaleDetail(key ?? ""),
    queryFn: () => unwrap(commands.quickSaleDetail(key ?? "")),
    enabled: key !== null,
  });
}

function useQuickSaleMutation<TArgs, TResult>(fn: (args: TArgs) => Promise<TResult>) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["quickSales"] });
      await qc.invalidateQueries({ queryKey: ["catalog"] });
    },
  });
}

export function usePromoteQuickSale() {
  return useQuickSaleMutation((input: PromoteInput) => unwrap(commands.promoteQuickSale(input)));
}

export function useIgnoreQuickSale() {
  return useQuickSaleMutation((key: string) => unwrap(commands.ignoreQuickSale(key)));
}

/** The till's "now a product" hint for what is typed in the Quick Sale bar (null name = nothing typed). */
export function useQuickSaleHint(name: string | null) {
  return useQuery({
    queryKey: queryKeys.quickSaleHint(name ?? ""),
    queryFn: () => unwrap(commands.quickSaleHint(name ?? "")),
    enabled: name !== null && name.trim() !== "",
    staleTime: 30_000,
  });
}

/** Today's business date on this counter (the default for report periods). */
export function useBusinessToday() {
  return useQuery({
    queryKey: queryKeys.businessToday,
    queryFn: () => unwrap(commands.businessToday()),
    staleTime: 60_000,
  });
}

// ───────── customer back office (doc §3.8, CUSTOMER_VIEW) ─────────

export function useCustomerList(search: string, sort: CustomerSort) {
  return useQuery({
    queryKey: queryKeys.customerList(search, sort),
    queryFn: () => unwrap(commands.customerList(search.trim() === "" ? null : search, sort)),
  });
}

export function useCustomerProfile(id: string | null) {
  return useQuery({
    queryKey: queryKeys.customerProfile(id ?? ""),
    queryFn: () => unwrap(commands.customerProfile(id ?? "")),
    enabled: id !== null,
  });
}

const HISTORY_PAGE = 100;

export function useCustomerHistory(id: string | null) {
  return useInfiniteQuery({
    queryKey: queryKeys.customerHistory(id ?? ""),
    initialPageParam: null as HistoryCursor | null,
    queryFn: ({ pageParam }) => unwrap(commands.customerHistory(id ?? "", pageParam, HISTORY_PAGE)),
    getNextPageParam: (last) => last.next_cursor ?? null,
    enabled: id !== null,
  });
}

export function useCustomerReceipt(customerId: string, orderId: string | null) {
  return useQuery({
    queryKey: queryKeys.customerReceipt(customerId, orderId ?? ""),
    queryFn: () => unwrap(commands.customerOrderReceipt(customerId, orderId ?? "")),
    enabled: orderId !== null,
  });
}

const TOP_CUSTOMERS = 10;

export function useCustomerAnalytics(period: Period | null, granularity: Granularity) {
  return useQuery({
    queryKey: queryKeys.customerAnalytics(period ?? { kind: "all_time" }, granularity),
    queryFn: () =>
      unwrap(commands.customerAnalytics(period ?? { kind: "all_time" }, granularity, TOP_CUSTOMERS)),
    enabled: period !== null,
  });
}

function useCustomerMutation<TArgs, TResult>(fn: (args: TArgs) => Promise<TResult>) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["customers"] });
      await qc.invalidateQueries({ queryKey: queryKeys.audit });
    },
  });
}

export function useUpdateCustomer() {
  return useCustomerMutation(({ id, input }: { id: string; input: CustomerUpdate }) =>
    unwrap(commands.updateCustomer(id, input)),
  );
}

export function useEraseCustomer() {
  return useCustomerMutation((id: string) => unwrap(commands.eraseCustomer(id)));
}

// ───────── expenses (doc §5.1, EXPENSE_MANAGE) ─────────

export function useExpenseCategories() {
  return useQuery({
    queryKey: queryKeys.expenseCategories,
    queryFn: () => unwrap(commands.expenseCategories()),
  });
}

export function useExpenseList(period: Period, categoryId: string | null) {
  return useQuery({
    queryKey: queryKeys.expenseList(period, categoryId),
    queryFn: () => unwrap(commands.expenseList(period, categoryId)),
  });
}

function useExpenseMutation<TArgs, TResult>(fn: (args: TArgs) => Promise<TResult>) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["expenses"] });
      await qc.invalidateQueries({ queryKey: ["cash"] });
      await qc.invalidateQueries({ queryKey: queryKeys.audit });
    },
  });
}

export function useSaveExpenseCategory() {
  return useExpenseMutation(({ id, input }: { id: string | null; input: ExpenseCategoryInput }) =>
    unwrap(commands.saveExpenseCategory(id, input)),
  );
}

export function useRecordExpense() {
  return useExpenseMutation(({ input, approvalId }: { input: ExpenseInput; approvalId: string | null }) =>
    unwrap(commands.recordExpense(input, approvalId)),
  );
}

export function useUpdateExpense() {
  return useExpenseMutation(({ id, input }: { id: string; input: ExpenseInput }) =>
    unwrap(commands.updateExpense(id, input)),
  );
}

export function useDeleteExpense() {
  return useExpenseMutation(({ id, approvalId }: { id: string; approvalId: string | null }) =>
    unwrap(commands.deleteExpense(id, approvalId)),
  );
}
