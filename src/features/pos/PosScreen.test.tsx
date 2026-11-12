// The till end to end against mocked commands: scan → cart, F2 quick sale, F4 phone, F9 pay with change,
// the supervisor step-up on an above-limit discount, an unknown barcode, hold, and the debounced save.
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import {
  commands,
  type CashSession_Serialize as CashSession,
  type Perm,
  type ProductSummary_Serialize as ProductSummary,
} from "@/db/bindings";
import { err, ok, renderWithProviders, session } from "@/test/render";
import { PosScreen, SAVE_DEBOUNCE_MS } from "./PosScreen";
import { cartStore } from "./store/cart.store";

vi.mock("@/lib/audio", () => ({ playCue: vi.fn(), setAudioPrefs: vi.fn() }));
vi.mock("@/db/bindings", () => ({
  commands: {
    currentCashSession: vi.fn(),
    searchProducts: vi.fn(),
    lookupProductCode: vi.fn(),
    customerLookup: vi.fn(),
    checkoutOrder: vi.fn(),
    cartSaveCurrent: vi.fn(),
    cartDiscardCurrent: vi.fn(),
    cartCurrent: vi.fn(),
    cartHold: vi.fn(),
    heldCarts: vi.fn(),
    cartRecall: vi.fn(),
    sessionOrders: vi.fn(),
    voidOrder: vi.fn(),
    listLoginStaff: vi.fn(),
    requestApproval: vi.fn(),
    quickSaleHint: vi.fn(),
    devicePreferences: vi.fn(),
  },
}));

const OPEN: CashSession = {
  id: "cs1",
  device_id: "dev1",
  business_date: "2026-11-08",
  status: "open",
  opened_by: "cashier",
  opened_by_name: "Nimal",
  opened_at: "2026-11-08T03:30:00.000Z",
  opening_float: 0,
  closed_by: null,
  closed_by_name: null,
  closed_at: null,
  expected_cash: null,
  counted_cash: null,
  variance: null,
  close_note: null,
};

const BUCKET: ProductSummary = {
  id: "p1",
  kind: "standard",
  name: "Blue Bucket",
  name_si: "නිල් බාල්දිය",
  sku: "100001",
  barcode: "4791234567890",
  price: 45_000,
  min_price: null,
  track_stock: true,
};

function As({ perms, children }: { perms: Perm[]; children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: perms })), [setSession, perms]);
  return <>{children}</>;
}

async function renderTill() {
  renderWithProviders(
    <As perms={["SALE_CREATE", "QUICK_SALE"]}>
      <PosScreen />
    </As>,
  );
  return screen.findByLabelText(/Scan or search/);
}

/** A HID scanner configured with the F13 prefix (deterministic capture, doc §7.1). */
async function scan(code: string) {
  const press = (c: string, k: string) =>
    window.dispatchEvent(new KeyboardEvent("keydown", { code: c, key: k, bubbles: true, cancelable: true }));
  await act(async () => {
    press("F13", "F13");
    for (const c of code) press(`Digit${c}`, c);
    press("Enter", "Enter");
    await new Promise((r) => setTimeout(r, 20));
  });
}

function key(code: string) {
  fireEvent.keyDown(window, { code, key: code });
}

beforeAll(() => {
  // jsdom has no layout: give the cart's scroll viewport a real size so the virtualiser renders lines.
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    width: 800,
    height: 480,
    top: 0,
    left: 0,
    bottom: 480,
    right: 800,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  });
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 480 });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 800 });
});

beforeEach(() => {
  cartStore.getState().clear();
  vi.mocked(commands.devicePreferences).mockImplementation(() =>
    ok({ default_language: "en", theme: "light", touch_mode: false }),
  );
  vi.mocked(commands.currentCashSession).mockImplementation(() => ok(OPEN));
  vi.mocked(commands.searchProducts).mockImplementation(() => ok([BUCKET]));
  vi.mocked(commands.lookupProductCode).mockImplementation((c: string) =>
    ok(c === BUCKET.barcode ? BUCKET : null),
  );
  vi.mocked(commands.customerLookup).mockImplementation(() =>
    ok({
      customer_id: "cu1",
      phone_e164: "+94771234567",
      phone_display: "077 123 4567",
      display_name: "Saman",
      visit_count: 4,
    }),
  );
  vi.mocked(commands.cartSaveCurrent).mockImplementation(() => ok(null));
  vi.mocked(commands.cartDiscardCurrent).mockImplementation(() => ok(null));
  vi.mocked(commands.cartCurrent).mockImplementation(() => ok(null));
  vi.mocked(commands.cartHold).mockImplementation(() =>
    ok({ id: "h1", label: null, payload: "{}", checkout_token: null, created_at: "", updated_at: "" }),
  );
  vi.mocked(commands.heldCarts).mockImplementation(() => ok([]));
  vi.mocked(commands.checkoutOrder).mockReset();
  vi.mocked(commands.quickSaleHint).mockImplementation(() => ok(null));
  vi.mocked(commands.listLoginStaff).mockImplementation(() =>
    ok([
      { id: "cashier", display_name: "Nimal", role_name: "Cashier" },
      { id: "sup", display_name: "Kamal", role_name: "Supervisor" },
    ]),
  );
  vi.mocked(commands.requestApproval).mockImplementation(() =>
    ok({ approval_id: "appr-1", expires_at: "2026-11-08T09:01:00.000Z" }),
  );
});

afterEach(() => vi.useRealTimers());

describe("PosScreen", () => {
  it("hides the on-screen keypad unless touch screen mode is on", async () => {
    await renderTill();
    await waitFor(() => expect(commands.devicePreferences).toHaveBeenCalled());
    expect(screen.queryByRole("group", { name: "Keypad" })).not.toBeInTheDocument();
  });

  it("shows the on-screen keypad in touch screen mode", async () => {
    vi.mocked(commands.devicePreferences).mockImplementation(() =>
      ok({ default_language: "en", theme: "light", touch_mode: true }),
    );
    await renderTill();
    expect(await screen.findByRole("group", { name: "Keypad" })).toBeInTheDocument();
  });

  it("adds a scanned product, bumps it on a second scan and totals it", async () => {
    await renderTill();
    await scan("4791234567890");
    await scan("4791234567890");
    await waitFor(() => expect(cartStore.getState().lines[0]?.qty).toBe(2));
    expect(screen.getByTestId("grand-total")).toHaveTextContent("Rs 900.00");
    expect(screen.getByText("Blue Bucket")).toBeInTheDocument();
  });

  it("adds a typed code on Enter in the scan field", async () => {
    const field = await renderTill();
    fireEvent.change(field, { target: { value: "4791234567890" } });
    fireEvent.keyDown(field, { key: "Enter", code: "Enter" });
    await waitFor(() => expect(cartStore.getState().lines).toHaveLength(1));
    expect(field).toHaveValue("");
  });

  it("toasts an unknown barcode and pre-fills Quick Sale, never a modal", async () => {
    await renderTill();
    await scan("2000123456781");
    expect(await screen.findByText("Not found: 2000123456781")).toBeInTheDocument();
    expect(screen.getByLabelText(/Quick sale/)).toHaveValue("2000123456781 ");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("F2 → type → Enter adds a quick sale with the live preview", async () => {
    await renderTill();
    key("F2");
    const bar = screen.getByLabelText(/Quick sale/);
    expect(bar).toHaveFocus();
    fireEvent.change(bar, { target: { value: "3* Bucket 10L 450" } });
    expect(screen.getByText("Bucket 10L · Rs 450.00 × 3")).toBeInTheDocument();
    fireEvent.keyDown(bar, { key: "Enter", code: "Enter" });
    expect(cartStore.getState().lines[0]).toMatchObject({ productId: null, name: "Bucket 10L", qty: 3 });
    expect(screen.getByTestId("grand-total")).toHaveTextContent("Rs 1,350.00");
    fireEvent.change(bar, { target: { value: "Bucket 10L" } });
    expect(screen.getByText(/Price missing/)).toBeInTheDocument();
  });

  it("hints that a typed quick sale is now a product; ↓ Enter adds the product", async () => {
    vi.mocked(commands.quickSaleHint).mockImplementation(() => ok({ product: BUCKET }));
    await renderTill();
    key("F2");
    const bar = screen.getByLabelText(/Quick sale/);
    fireEvent.change(bar, { target: { value: "Blue Bucket 450" } });
    expect(await screen.findByText(/Now a product: Blue Bucket/)).toBeInTheDocument();
    fireEvent.keyDown(bar, { key: "ArrowDown", code: "ArrowDown" });
    fireEvent.keyDown(bar, { key: "Enter", code: "Enter" });
    expect(cartStore.getState().lines[0]).toMatchObject({ productId: "p1", name: "Blue Bucket" });
  });

  it("plain Enter still commits the typed quick sale when a hint shows", async () => {
    vi.mocked(commands.quickSaleHint).mockImplementation(() => ok({ product: BUCKET }));
    await renderTill();
    key("F2");
    const bar = screen.getByLabelText(/Quick sale/);
    fireEvent.change(bar, { target: { value: "Blue Bucket 450" } });
    await screen.findByText(/Now a product/);
    fireEvent.keyDown(bar, { key: "Enter", code: "Enter" });
    expect(cartStore.getState().lines[0]).toMatchObject({ productId: null, name: "Blue Bucket" });
  });

  it("F4 shows a known customer by phone", async () => {
    await renderTill();
    key("F4");
    const phone = screen.getByLabelText(/Customer phone/);
    expect(phone).toHaveFocus();
    fireEvent.change(phone, { target: { value: "0771234567" } });
    expect(phone).toHaveValue("077 123 4567");
    expect(await screen.findByText("Saman · Visit #5")).toBeInTheDocument();
  });

  it("F9 pays in cash with change and starts a fresh cart", async () => {
    vi.mocked(commands.checkoutOrder).mockImplementation(() =>
      ok({
        order_id: "o1",
        receipt_no: "T01-20261108-0001",
        grand_total: 45_000,
        change_due: 5_000,
        customer_id: null,
        replayed: false,
      }),
    );
    await renderTill();
    act(() => cartStore.getState().addProduct(BUCKET));
    await screen.findByText("Blue Bucket");
    const token = cartStore.getState().token;
    key("F9");
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText(/Cash received/), { target: { value: "500" } });
    expect(within(dialog).getByText("Rs 50.00")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Complete sale" }));
    await waitFor(() => expect(commands.checkoutOrder).toHaveBeenCalledTimes(1));
    expect(vi.mocked(commands.checkoutOrder).mock.calls[0]?.[0]).toMatchObject({
      checkout_token: token,
      lines: [{ product_id: "p1", unit_price: 45_000, qty: 1, line_discount: 0 }],
      payments: [{ method: "cash", amount: 45_000, tendered: 50_000 }],
      approval_id: null,
    });
    expect(await screen.findByText(/Sale complete ✓/)).toHaveTextContent("Change Rs 50.00");
    expect(cartStore.getState().lines).toEqual([]);
    expect(cartStore.getState().token).not.toBe(token);
  });

  it("asks a supervisor when Rust requires approval, then retries with it", async () => {
    vi.mocked(commands.checkoutOrder)
      .mockImplementationOnce(() => err({ kind: "APPROVAL_REQUIRED", perm: "ORDER_DISCOUNT" }))
      .mockImplementationOnce(() =>
        ok({
          order_id: "o1",
          receipt_no: "T01-20261108-0001",
          grand_total: 42_300,
          change_due: 0,
          customer_id: null,
          replayed: false,
        }),
      );
    await renderTill();
    cartStore.getState().addProduct(BUCKET);
    cartStore.getState().setBillDiscount(2_700); // 6%: above the cashier's 5%
    expect(await screen.findByText(/Above your discount limit/)).toBeInTheDocument();
    key("F9");
    const pay = await screen.findByRole("dialog");
    fireEvent.click(within(pay).getByRole("button", { name: "Complete sale" }));
    fireEvent.click(await screen.findByRole("button", { name: "Kamal" }));
    fireEvent.change(screen.getByLabelText("PIN"), { target: { value: "2222" } });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    await waitFor(() => expect(commands.checkoutOrder).toHaveBeenCalledTimes(2));
    const token = vi.mocked(commands.checkoutOrder).mock.calls[0]?.[0].checkout_token;
    expect(vi.mocked(commands.requestApproval)).toHaveBeenCalledWith("ORDER_DISCOUNT", token, "sup", "2222");
    expect(vi.mocked(commands.checkoutOrder).mock.calls[1]?.[0]).toMatchObject({
      checkout_token: token,
      approval_id: "appr-1",
    });
  });

  it("F8 holds the sale and clears the till", async () => {
    await renderTill();
    cartStore.getState().addProduct(BUCKET);
    key("F8");
    await waitFor(() => expect(commands.cartHold).toHaveBeenCalledTimes(1));
    expect(JSON.parse(vi.mocked(commands.cartHold).mock.calls[0]?.[0].payload ?? "{}")).toMatchObject({
      v: 1,
      lines: [{ productId: "p1" }],
    });
    await waitFor(() => expect(cartStore.getState().lines).toEqual([]));
  });

  it("saves the cart in progress for crash recovery, debounced", async () => {
    await renderTill();
    vi.useFakeTimers();
    act(() => {
      cartStore.getState().addProduct(BUCKET);
      cartStore.getState().addProduct(BUCKET);
    });
    expect(commands.cartSaveCurrent).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SAVE_DEBOUNCE_MS + 10);
    });
    expect(commands.cartSaveCurrent).toHaveBeenCalledTimes(1);
    expect(vi.mocked(commands.cartSaveCurrent).mock.calls[0]?.[0].checkout_token).toBe(
      cartStore.getState().token,
    );
  });

  it("offers to recover a cart left from before a restart", async () => {
    cartStore.getState().addProduct(BUCKET);
    const payload = JSON.stringify(cartStore.getState().snapshot());
    cartStore.getState().clear();
    vi.mocked(commands.cartCurrent).mockImplementation(() =>
      ok({ id: "__current__", label: null, payload, checkout_token: "t", created_at: "", updated_at: "" }),
    );
    await renderTill();
    fireEvent.click(await screen.findByRole("button", { name: "Recover cart" }));
    expect(cartStore.getState().lines).toHaveLength(1);
  });

  it("shows the shortcut sheet on F1", async () => {
    await renderTill();
    key("F1");
    expect(await screen.findByRole("dialog", { name: "Till shortcuts" })).toBeInTheDocument();
  });
});
