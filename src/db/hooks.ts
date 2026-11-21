// TanStack Query hooks over the generated commands. Only src/db and src/hardware call Rust (AGENT.md §2).
import {
  keepPreviousData,
  useInfiniteQuery,
  useMutation,
  useQueries,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
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
  type Logo,
  type ReceiptImage,
  type LateItemInput,
  type CheckoutInput_Deserialize as CheckoutInput,
  type CloseInput,
  type CustomerSort,
  type CustomerUpdate,
  type ExpenseCategoryInput,
  type ExpenseInput,
  type ExportKind,
  type OrderCursor,
  type OrderFilter_Deserialize as OrderFilter,
  type Granularity,
  type HistoryCursor,
  type Period,
  type SalesDimension,
  type DrawerMovementInput,
  type HistoryFilter,
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
  type BootstrapProgress,
  type RecoverInput,
  type JoinInput_Deserialize as JoinInput,
  type RegisterInput_Deserialize as RegisterInput,
  type ResolveDeadOp,
  type SyncStatus_Serialize as SyncStatus,
} from "./bindings";
import { unwrap } from "./client";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

export const queryKeys = {
  status: ["app", "status"] as const,
  devicePrefs: ["app", "device-prefs"] as const,
  appInfo: ["app", "info"] as const,
  integrity: ["app", "integrity"] as const,
  loginStaff: ["auth", "login-staff"] as const,
  staffSales: (id: string) => ["staff", "sales", id] as const,
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
  receiptHeader: ["printer", "header"] as const,
  systemPrinters: ["system-printers"] as const,
  customer: (phone: string) => ["customers", "lookup", phone] as const,
  customerList: (search: string, sort: CustomerSort) => ["customers", "list", search, sort] as const,
  customerProfile: (id: string) => ["customers", "profile", id] as const,
  customerHistory: (id: string) => ["customers", "history", id] as const,
  customerReceipt: (customerId: string, orderId: string) =>
    ["customers", "receipt", customerId, orderId] as const,
  customerAnalytics: (period: Period, g: Granularity) =>
    ["customers", "analytics", JSON.stringify(period), g] as const,
  businessToday: ["app", "today"] as const,
  reportSummary: (period: Period, deviceId: string | null, compare: boolean) =>
    ["reports", "summary", JSON.stringify(period), deviceId ?? "store", compare] as const,
  reportTrend: (period: Period, deviceId: string | null, g: Granularity, compare: boolean) =>
    ["reports", "trend", JSON.stringify(period), deviceId ?? "store", g, compare] as const,
  reportSellThrough: ["reports", "sellThrough"] as const,
  reportSalesBy: (period: Period, deviceId: string | null, d: SalesDimension) =>
    ["reports", "salesBy", JSON.stringify(period), deviceId ?? "store", d] as const,
  reportVoids: (period: Period, deviceId: string | null) =>
    ["reports", "voids", JSON.stringify(period), deviceId ?? "store"] as const,
  reportDevices: ["reports", "devices"] as const,
  orderHistory: (filter: OrderFilter) => ["orders", "history", JSON.stringify(filter)] as const,
  orderReceipt: (id: string) => ["orders", "receipt", id] as const,
  expenseCategories: ["expenses", "categories"] as const,
  expenseList: (period: Period, categoryId: string | null) =>
    ["expenses", "list", JSON.stringify(period), categoryId ?? "all"] as const,
  sessionOrders: ["orders", "session"] as const,
  heldCarts: ["pos", "held"] as const,
  currentCart: ["pos", "current"] as const,
  backupStatus: ["backup", "status"] as const,
  syncStatus: ["sync", "status"] as const,
  syncInfo: ["sync", "info"] as const,
  syncDeadOps: ["sync", "dead"] as const,
  syncLogs: ["sync", "logs"] as const,
  restoreStatus: ["restore", "status"] as const,
  restoreCandidates: ["restore", "candidates"] as const,
  staff: ["staff", "list"] as const,
  labelTemplates: ["labels", "templates"] as const,
  valuation: (asOf: string | null) => ["inventory", "valuation", asOf ?? "now"] as const,
  oversold: ["inventory", "oversold"] as const,
  quickSales: (show: QuickSaleStatus | null) => ["quickSales", "review", show ?? "all"] as const,
  quickSaleDetail: (key: string) => ["quickSales", "detail", key] as const,
  quickSaleHint: (name: string) => ["quickSales", "hint", name] as const,
  movements: (productId: string) => ["inventory", "movements", productId] as const,
  labelProfile: ["labels", "profile"] as const,
  labelSample: (templateId: string) => ["labels", "sample", templateId] as const,
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

/** This counter's touch screen mode: on-screen keypads and number pads show only when it is on. */
export function useTouchMode(): boolean {
  return useDevicePrefs().data?.touch_mode ?? false;
}

export function useSetDeviceTouchMode() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (enabled: boolean) => unwrap(commands.setDeviceTouchMode(enabled)),
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

/** Shift history for the Cash drawer's History tab (own counter, or every counter with REPORT_SALES). */
export function useCashHistory(filter: HistoryFilter, limit: number) {
  return useQuery({
    queryKey: ["cash", "history", filter, limit] as const,
    queryFn: () => unwrap(commands.cashHistory(filter, limit)),
    placeholderData: keepPreviousData,
  });
}

/** The Z-report of a closed shift, or the live X-report of the open one. */
export function useCashSessionReport(sessionId: string | null) {
  return useQuery({
    queryKey: ["cash", "report", sessionId] as const,
    queryFn: () => unwrap(commands.cashSessionReport(sessionId ?? "")),
    enabled: sessionId !== null,
  });
}

export function useReprintZReport() {
  return useMutation({ mutationFn: (sessionId: string) => unwrap(commands.reprintZReport(sessionId)) });
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

/** The printers installed in Windows, for the Settings dropdowns; `refetch()` is the Refresh button. */
export function useSystemPrinters() {
  return useQuery({
    queryKey: queryKeys.systemPrinters,
    queryFn: () => unwrap(commands.listSystemPrinters()),
    staleTime: 30_000,
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

export function useReceiptHeader() {
  return useQuery({ queryKey: queryKeys.receiptHeader, queryFn: () => unwrap(commands.receiptHeader()) });
}

export function useSetReceiptLogo() {
  return usePrinterMutation((logo: Logo | null) => unwrap(commands.setReceiptLogo(logo)));
}

export function usePrintTestPage() {
  return usePrinterMutation(() => unwrap(commands.printTestPage()));
}

/** `orderId` marks a reprint of that bill (audited); `null` is a sample such as the Settings test receipt. */
export function usePrintReceiptImage() {
  return usePrinterMutation(({ image, orderId }: { image: ReceiptImage; orderId: string | null }) =>
    unwrap(commands.printReceiptImage(image, orderId)),
  );
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

/** Exchange step 1: look the earlier sale up by receipt number (no cache: the answer is per attempt). */
export function useExchangeLookup() {
  return useMutation({ mutationFn: (receiptNo: string) => unwrap(commands.exchangeLookup(receiptNo)) });
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

/** A scanned or typed barcode / SKU: every matching active product (variants may share a barcode). */
export function lookupProductMatches(code: string) {
  return unwrap(commands.lookupProductCodeMatches(code));
}

/** Category and stock of a few products the till is asking about (the disambiguation dialog). */
export function useProductDetails(ids: readonly string[]) {
  return useQueries({
    queries: ids.map((id) => ({
      queryKey: ["catalog", "product", id] as const,
      queryFn: () => unwrap(commands.getProduct(id)),
      staleTime: 10_000,
    })),
  });
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

/** One person's daily sales for the chart and their week, month and year totals (Staff → Sales performance). */
export function useStaffSales(id: string | null) {
  return useQuery({
    queryKey: queryKeys.staffSales(id ?? ""),
    queryFn: () => unwrap(commands.staffSales(id ?? "")),
    enabled: id !== null,
  });
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

/** One sample sticker row on a roll, laid out by Rust exactly as it prints. */
export function useLabelSamplePreview(templateId: string | null) {
  return useQuery({
    queryKey: queryKeys.labelSample(templateId ?? ""),
    queryFn: () => unwrap(commands.labelSamplePreview(templateId ?? "")),
    enabled: templateId !== null,
    retry: false,
    placeholderData: keepPreviousData,
  });
}

export function useLabelSamplePrint() {
  return useLabelMutation((templateId: string) => unwrap(commands.labelSamplePrint(templateId)));
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
      await qc.invalidateQueries({ queryKey: ["catalog"] });
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

// ───────── reports (doc §5, REPORT_SALES; costs need REPORT_FINANCIAL + VIEW_COST) ─────────

export function useReportSummary(period: Period, deviceId: string | null, compare: boolean) {
  return useQuery({
    queryKey: queryKeys.reportSummary(period, deviceId, compare),
    queryFn: () => unwrap(commands.reportSummary(period, deviceId, compare)),
  });
}

export function useReportTrend(
  period: Period,
  deviceId: string | null,
  granularity: Granularity,
  compare: boolean,
) {
  return useQuery({
    queryKey: queryKeys.reportTrend(period, deviceId, granularity, compare),
    queryFn: () => unwrap(commands.reportTrend(period, deviceId, granularity, compare)),
  });
}

export function useReportSellThrough() {
  return useQuery({
    queryKey: queryKeys.reportSellThrough,
    queryFn: () => unwrap(commands.reportBatchSellThrough()),
  });
}

export function useReportSalesBy(period: Period, deviceId: string | null, dimension: SalesDimension) {
  return useQuery({
    queryKey: queryKeys.reportSalesBy(period, deviceId, dimension),
    queryFn: () => unwrap(commands.reportSalesBy(period, deviceId, dimension)),
  });
}

export function useReportVoids(period: Period, deviceId: string | null) {
  return useQuery({
    queryKey: queryKeys.reportVoids(period, deviceId),
    queryFn: () => unwrap(commands.reportVoids(period, deviceId)),
  });
}

export function useReportDevices() {
  return useQuery({ queryKey: queryKeys.reportDevices, queryFn: () => unwrap(commands.reportDevices()) });
}

/** Settings → Maintenance: recompute every derived table (then every screen refreshes). */
export function useRebuildDerived() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => unwrap(commands.rebuildDerived()),
    onSuccess: () => qc.invalidateQueries(),
  });
}

// ───────── order history and CSV export (doc §1.4, §5) ─────────

const ORDER_PAGE = 200;

export function useOrderHistory(filter: OrderFilter) {
  return useInfiniteQuery({
    queryKey: queryKeys.orderHistory(filter),
    initialPageParam: null as OrderCursor | null,
    queryFn: ({ pageParam }) => unwrap(commands.orderHistory(filter, pageParam, ORDER_PAGE)),
    getNextPageParam: (last) => last.next_cursor ?? null,
  });
}

export function useOrderReceipt(orderId: string) {
  return useQuery({
    queryKey: queryKeys.orderReceipt(orderId),
    queryFn: () => unwrap(commands.orderReceipt(orderId)),
  });
}

/** The native folder picker for exports; `null` when cancelled. */
export function pickExportFolder() {
  return unwrap(commands.exportPickFolder());
}

export function useExportCsv() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ kind, period, dir }: { kind: ExportKind; period: Period; dir: string }) =>
      unwrap(commands.exportCsv(kind, period, dir)),
    onSuccess: () => qc.invalidateQueries({ queryKey: queryKeys.audit }),
  });
}

// ───────── cloud sync (doc §1.3, §6) ─────────

/** Run `onEvent` for each payload of a Rust event. Outside Tauri (tests, a plain browser) there is no bus. */
function useRustEvent<T>(name: string, onEvent: (payload: T) => void) {
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    listen<T>(name, (event) => onEvent(event.payload))
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {
        // Not running inside Tauri.
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [name, onEvent]);
}

/** The sync worker's status: read once, then kept current by `sync://status`. */
export function useSyncStatus() {
  const qc = useQueryClient();
  const [onStatus] = useState(() => (status: SyncStatus) => qc.setQueryData(queryKeys.syncStatus, status));
  useRustEvent("sync://status", onStatus);
  return useQuery({
    queryKey: queryKeys.syncStatus,
    queryFn: () => unwrap(commands.syncStatus()),
    refetchInterval: 30_000,
  });
}

export function useSyncInfo() {
  return useQuery({ queryKey: queryKeys.syncInfo, queryFn: () => unwrap(commands.syncInfo()) });
}

export function useSyncNow() {
  return useMutation({ mutationFn: () => unwrap(commands.syncNow()) });
}

export function useSyncRegister() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: RegisterInput) => unwrap(commands.syncRegister(input)),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sync"] }),
  });
}

/** Only mounted for `SYNC_MANAGE` (Rust refuses anyone else). */
export function useSyncDeadOps() {
  return useQuery({ queryKey: queryKeys.syncDeadOps, queryFn: () => unwrap(commands.syncDeadOps()) });
}

export function useResolveDeadOp() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: ResolveDeadOp) => unwrap(commands.syncResolveDeadOp(input)),
    onSettled: () => qc.invalidateQueries({ queryKey: ["sync"] }),
  });
}

export function useSyncLogs(limit: number) {
  return useQuery({
    queryKey: [...queryKeys.syncLogs, limit],
    queryFn: () => unwrap(commands.syncLogs(limit)),
  });
}

/** The latest `sync://bootstrap` progress while a bootstrap runs. */
export function useBootstrapProgress(): BootstrapProgress | null {
  const [progress, setProgress] = useState<BootstrapProgress | null>(null);
  const [onProgress] = useState(() => (p: BootstrapProgress) => setProgress(p));
  useRustEvent("sync://bootstrap", onProgress);
  return progress;
}

/** First-run "Join existing store": register, then pull the store's history. */
export function useJoinStore() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: JoinInput) => unwrap(commands.setupJoinStore(input)),
    onSettled: () => qc.invalidateQueries({ queryKey: queryKeys.status }),
  });
}

/** Resume an interrupted bootstrap. */
export function useResumeBootstrap() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => unwrap(commands.syncBootstrap()),
    onSettled: () => qc.invalidateQueries({ queryKey: queryKeys.status }),
  });
}

// ───────── restore (doc §8 Phase 4, Block 4.1) ─────────

/** Whether a restored backup awaits recovery, and the recovery paths open to this counter. */
export function useRestoreStatus(enabled = true) {
  return useQuery({
    queryKey: queryKeys.restoreStatus,
    queryFn: () => unwrap(commands.restoreStatus()),
    enabled,
  });
}

/** The automatic, pre-restore and last manual backups on this counter (`BACKUP_RESTORE`). */
export function useRestoreCandidates() {
  return useQuery({
    queryKey: queryKeys.restoreCandidates,
    queryFn: () => unwrap(commands.restoreCandidates()),
  });
}

/** The native file picker for a backup file; `null` when cancelled. */
export function pickRestoreFile() {
  return unwrap(commands.restorePickFile());
}

/** Check the file, back up the current database, stage the restore. The app restarts by itself. */
export function useRestoreStage() {
  return useMutation({ mutationFn: (path: string) => unwrap(commands.restoreStage(path)) });
}

/** First run on a new PC: restore this counter from its backup file. The app restarts by itself. */
export function useRestoreStageFresh() {
  return useMutation({ mutationFn: (path: string) => unwrap(commands.restoreStageFresh(path)) });
}

/** Finish a restore with the Owner's PIN. */
export function useRestoreRecover() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: RecoverInput) => unwrap(commands.restoreRecover(input)),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: queryKeys.status });
      void qc.invalidateQueries({ queryKey: ["restore"] });
    },
  });
}
