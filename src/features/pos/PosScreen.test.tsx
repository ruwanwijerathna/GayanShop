// The till end to end against mocked commands: scan → cart, F2 quick sale, F4 phone, F9 pay with change,
// the supervisor step-up on an above-limit discount, an unknown barcode, hold, and the debounced save.
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect, type ReactNode } from "react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import {
  commands,
  type CashSession_Serialize as CashSession,
  type ExchangeSource,
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
    lookupProductCodeMatches: vi.fn(),
    getProduct: vi.fn(),
    listCategories: vi.fn(),
    customerLookup: vi.fn(),
    checkoutOrder: vi.fn(),
    exchangeLookup: vi.fn(),
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

const SHARED = "4792024015103";
const LIME: ProductSummary = {
  ...BUCKET,
  id: "p2",
  name: "Lime Soap",
  sku: "100002",
  barcode: SHARED,
  price: 12_000,
};
const MINT: ProductSummary = {
  ...BUCKET,
  id: "p3",
  name: "Mint Soap",
  sku: "100003",
  barcode: SHARED,
  price: 13_000,
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
  vi.mocked(commands.lookupProductCodeMatches).mockImplementation((c: string) => {
    if (c === BUCKET.barcode) return ok([BUCKET]);
    if (c === SHARED) return ok([LIME, MINT]);
    return ok([]);
  });
  vi.mocked(commands.listCategories).mockImplementation(() =>
    ok([{ id: "c1", name: "Soap", parent_id: null, product_count: 2 } as never]),
  );
  vi.mocked(commands.getProduct).mockImplementation((id: string) =>
    ok({ id, category_id: "c1", on_hand: id === "p2" ? 12 : 3 } as never),
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

describe("PosScreen: shared barcodes", () => {
  it("adds a unique barcode straight away, with no dialog", async () => {
    await renderTill();
    await scan("4791234567890");
    await waitFor(() => expect(cartStore.getState().lines).toHaveLength(1));
    expect(screen.queryByRole("listbox", { name: "Select product" })).not.toBeInTheDocument();
  });

  it("asks which variant when a scanned barcode matches several, and adds the one chosen with ↓ Enter", async () => {
    await renderTill();
    await scan(SHARED);
    const list = await screen.findByRole("listbox", { name: "Select product" });
    await waitFor(() => expect(list).toHaveFocus());
    const options = within(list).getAllByRole("option");
    expect(options.map((o) => o.getAttribute("aria-selected"))).toEqual(["true", "false"]);
    expect(options[0]).toHaveTextContent("Lime Soap");
    expect(await within(list).findByText(/Soap · 100002 · 12 in stock/)).toBeInTheDocument();
    expect(cartStore.getState().lines).toHaveLength(0);
    fireEvent.keyDown(list, { key: "ArrowDown" });
    expect(within(list).getAllByRole("option")[1]).toHaveAttribute("aria-selected", "true");
    fireEvent.keyDown(list, { key: "Enter" });
    await waitFor(() => expect(cartStore.getState().lines.map((l) => l.name)).toEqual(["Mint Soap"]));
    expect(screen.queryByRole("listbox", { name: "Select product" })).not.toBeInTheDocument();
  });

  it("ArrowUp stops at the first row, and Esc adds nothing", async () => {
    await renderTill();
    await scan(SHARED);
    const list = await screen.findByRole("listbox", { name: "Select product" });
    fireEvent.keyDown(list, { key: "ArrowUp" });
    expect(within(list).getAllByRole("option")[0]).toHaveAttribute("aria-selected", "true");
    fireEvent.keyDown(list, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("listbox", { name: "Select product" })).not.toBeInTheDocument(),
    );
    expect(cartStore.getState().lines).toHaveLength(0);
  });

  it("lets a click pick a row", async () => {
    await renderTill();
    await scan(SHARED);
    const list = await screen.findByRole("listbox", { name: "Select product" });
    fireEvent.mouseDown(within(list).getAllByRole("option")[1] as HTMLElement);
    await waitFor(() => expect(cartStore.getState().lines.map((l) => l.name)).toEqual(["Mint Soap"]));
  });

  it("does the same for a shared barcode typed into the scan field", async () => {
    const field = await renderTill();
    fireEvent.change(field, { target: { value: SHARED } });
    fireEvent.keyDown(field, { key: "Enter", code: "Enter" });
    expect(await screen.findByRole("listbox", { name: "Select product" })).toBeInTheDocument();
    expect(cartStore.getState().lines).toHaveLength(0);
  });
});

describe("PosScreen", () => {
  it("keeps the till simple: no on-screen keypad, even in touch screen mode", async () => {
    vi.mocked(commands.devicePreferences).mockImplementation(() =>
      ok({ default_language: "en", theme: "light", touch_mode: true }),
    );
    await renderTill();
    expect(screen.queryByRole("group", { name: "Keypad" })).not.toBeInTheDocument();
  });

  it("shows only Pay, Exchange and More until the menu is opened", async () => {
    await renderTill();
    expect(screen.getByRole("button", { name: /PAY/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Exchange/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "More" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Hold/ })).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Quick sale/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Customer phone/)).not.toBeInTheDocument();
  });

  it("opens Hold and Quick sale from the More menu", async () => {
    await renderTill();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "More" }));
    await user.click(await screen.findByRole("menuitem", { name: /Quick sale/ }));
    expect(await screen.findByLabelText(/Quick sale/)).toHaveFocus();
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
    // The panel closes after an add and focus is back on the scan field; F2 reopens it.
    expect(screen.queryByLabelText(/Quick sale/)).not.toBeInTheDocument();
    key("F2");
    fireEvent.change(screen.getByLabelText(/Quick sale/), { target: { value: "Bucket 10L" } });
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

  it("shows a known customer by phone in payment dialog", async () => {
    await renderTill();
    act(() => cartStore.getState().addProduct(BUCKET));
    key("F9");
    const dialog = await screen.findByRole("dialog");
    const phone = within(dialog).getByLabelText(/Customer phone/);
    fireEvent.change(phone, { target: { value: "0771234567" } });
    expect(phone).toHaveValue("077 123 4567");
    expect(await within(dialog).findByText("Saman · Visit #5")).toBeInTheDocument();
  });

  it("F9 pays in cash with change and starts a fresh cart", async () => {
    vi.mocked(commands.checkoutOrder).mockImplementation(() =>
      ok({
        order_id: "o1",
        receipt_no: "T01-20261108-0001",
        grand_total: 45_000,
        change_due: 5_000,
        exchange_credit: 0,
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
    fireEvent.change(within(dialog).getByLabelText(/Cash received/i), { target: { value: "500" } });
    expect(within(dialog).getByText("Rs 50.00")).toBeInTheDocument();
    // Who made the sale is required: no number, no sale.
    const complete = within(dialog).getByRole("button", { name: "Pay Now" });
    expect(within(dialog).getByLabelText("Salesperson no.")).toHaveFocus();
    expect(within(dialog).getByLabelText("Salesperson no.")).toHaveValue("1");
    fireEvent.change(within(dialog).getByLabelText("Salesperson no."), { target: { value: "0" } });
    expect(complete).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Salesperson no."), { target: { value: "3a" } });
    expect(within(dialog).getByLabelText("Salesperson no.")).toHaveValue("3");
    fireEvent.click(complete);
    await waitFor(() => expect(commands.checkoutOrder).toHaveBeenCalledTimes(1));
    expect(vi.mocked(commands.checkoutOrder).mock.calls[0]?.[0]).toMatchObject({
      checkout_token: token,
      lines: [{ product_id: "p1", unit_price: 45_000, qty: 1, line_discount: 0 }],
      payments: [{ method: "cash", amount: 45_000, tendered: 50_000 }],
      sold_by_no: 3,
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
          exchange_credit: 0,
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
    fireEvent.change(within(pay).getByLabelText("Salesperson no."), { target: { value: "2" } });
    fireEvent.click(within(pay).getByRole("button", { name: "Pay Now" }));
    fireEvent.click(await screen.findByRole("button", { name: "Kamal" }));
    fireEvent.change(screen.getByLabelText("PIN"), { target: { value: "2222" } });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    await waitFor(() => expect(commands.checkoutOrder).toHaveBeenCalledTimes(2));
    const token = vi.mocked(commands.checkoutOrder).mock.calls[0]?.[0].checkout_token;
    expect(vi.mocked(commands.requestApproval)).toHaveBeenCalledWith("ORDER_DISCOUNT", token, "sup", "2222");
    expect(vi.mocked(commands.checkoutOrder).mock.calls[1]?.[0]).toMatchObject({
      checkout_token: token,
      sold_by_no: 2,
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

  it("shows the shortcut sheet on F1 with function keys and cart navigation shortcuts", async () => {
    await renderTill();
    key("F1");
    const dialog = await screen.findByRole("dialog", { name: "Till shortcuts" });
    expect(dialog).toBeInTheDocument();
    expect(within(dialog).getByText("F6")).toBeInTheDocument();
    expect(within(dialog).getByText("Delete")).toBeInTheDocument();
    expect(within(dialog).getByText("Ctrl+→")).toBeInTheDocument();
    expect(within(dialog).getByText("Ctrl+←")).toBeInTheDocument();
  });

  describe("exchange (F12)", () => {
    const SOURCE: ExchangeSource = {
      order_id: "o-old",
      receipt_no: "T01-20261105-0007",
      business_date: "2026-11-05",
      days_since: 3,
      window_days: 7,
      late: false,
      lines: [
        {
          item_id: "i1",
          name: "Blue Bucket",
          qty: 2,
          line_total: 90_000,
          returned_qty: 0,
          returnable_qty: 2,
          eligible: true,
        },
        {
          item_id: "i2",
          name: "Loose item",
          qty: 1,
          line_total: 5_000,
          returned_qty: 0,
          returnable_qty: 0,
          eligible: false,
        },
      ],
    };
    const DONE = {
      order_id: "o-new",
      receipt_no: "T01-20261108-0002",
      grand_total: 90_000,
      change_due: 0,
      exchange_credit: 45_000,
      customer_id: null,
      replayed: false,
    };

    /** F12 → find the sale → hand back one bucket → use it for the exchange. */
    async function startExchange(source: ExchangeSource = SOURCE) {
      vi.mocked(commands.exchangeLookup).mockImplementation(() => ok(source));
      await renderTill();
      key("F12");
      const dialog = await screen.findByRole("dialog", { name: "Exchange" });
      fireEvent.change(within(dialog).getByLabelText("Receipt number of the sale"), {
        target: { value: source.receipt_no },
      });
      fireEvent.click(within(dialog).getByRole("button", { name: "Find sale" }));
      await within(dialog).findByText(/sold 2026-11-05/);
      expect(within(dialog).getByText("Can't be returned (not a stocked item)")).toBeInTheDocument();
      fireEvent.click(within(dialog).getByRole("button", { name: "Return more Blue Bucket" }));
      expect(within(dialog).getByTestId("exchange-credit")).toHaveTextContent("Rs 450.00");
      fireEvent.click(within(dialog).getByRole("button", { name: "Use for exchange" }));
      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    }

    it("charges only the difference and sends the returned units with the replacement", async () => {
      vi.mocked(commands.checkoutOrder).mockImplementation(() => ok(DONE));
      await startExchange();
      act(() => {
        cartStore.getState().addProduct(BUCKET);
        cartStore.getState().addProduct(BUCKET);
      });
      expect(await screen.findByTestId("exchange-credit-line")).toHaveTextContent("Rs 450.00");
      expect(screen.getByTestId("exchange-to-pay")).toHaveTextContent("Rs 450.00");
      key("F9");
      const dialog = await screen.findByRole("dialog");
      fireEvent.change(within(dialog).getByLabelText("Salesperson no."), { target: { value: "1" } });
      fireEvent.click(within(dialog).getByRole("button", { name: "Pay Now" }));
      await waitFor(() => expect(commands.checkoutOrder).toHaveBeenCalledTimes(1));
      expect(vi.mocked(commands.checkoutOrder).mock.calls[0]?.[0]).toMatchObject({
        lines: [{ product_id: "p1", qty: 2 }],
        payments: [{ method: "cash", amount: 45_000 }],
        exchange: { returns: [{ item_id: "i1", qty: 1 }] },
      });
      expect(await screen.findByText(/Exchange complete ✓/)).toBeInTheDocument();
      expect(cartStore.getState().exchange).toBeNull();
    });

    it("blocks payment while the replacement is worth less than the returned goods", async () => {
      await startExchange();
      act(() => cartStore.getState().addProduct(LIME));
      expect(await screen.findByText(/must be worth at least Rs 450\.00/)).toBeInTheDocument();
      expect(screen.getByRole("button", { name: /PAY/ })).toBeDisabled();
      key("F9");
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

    it("an even swap sends no payment rows", async () => {
      vi.mocked(commands.checkoutOrder).mockImplementation(() => ok({ ...DONE, grand_total: 45_000 }));
      await startExchange();
      act(() => cartStore.getState().addProduct(BUCKET));
      expect(await screen.findByTestId("exchange-to-pay")).toHaveTextContent("Rs 0.00");
      key("F9");
      const dialog = await screen.findByRole("dialog");
      fireEvent.change(within(dialog).getByLabelText("Salesperson no."), { target: { value: "1" } });
      fireEvent.click(within(dialog).getByRole("button", { name: "Pay Now" }));
      await waitFor(() => expect(commands.checkoutOrder).toHaveBeenCalledTimes(1));
      expect(vi.mocked(commands.checkoutOrder).mock.calls[0]?.[0].payments).toEqual([]);
    });

    it("a late exchange asks a supervisor, then retries with the approval and the same returns", async () => {
      vi.mocked(commands.checkoutOrder)
        .mockImplementationOnce(() => err({ kind: "APPROVAL_REQUIRED", perm: "EXCHANGE_LATE" }))
        .mockImplementationOnce(() => ok(DONE));
      await startExchange({ ...SOURCE, days_since: 9, late: true });
      act(() => {
        cartStore.getState().addProduct(BUCKET);
        cartStore.getState().addProduct(BUCKET);
      });
      key("F9");
      const pay = await screen.findByRole("dialog");
      fireEvent.change(within(pay).getByLabelText("Salesperson no."), { target: { value: "2" } });
      fireEvent.click(within(pay).getByRole("button", { name: "Pay Now" }));
      fireEvent.click(await screen.findByRole("button", { name: "Kamal" }));
      fireEvent.change(screen.getByLabelText("PIN"), { target: { value: "2222" } });
      fireEvent.click(screen.getByRole("button", { name: "Approve" }));
      await waitFor(() => expect(commands.checkoutOrder).toHaveBeenCalledTimes(2));
      const token = vi.mocked(commands.checkoutOrder).mock.calls[0]?.[0].checkout_token;
      expect(vi.mocked(commands.requestApproval)).toHaveBeenCalledWith("EXCHANGE_LATE", token, "sup", "2222");
      expect(vi.mocked(commands.checkoutOrder).mock.calls[1]?.[0]).toMatchObject({
        approval_id: "appr-1",
        exchange: { returns: [{ item_id: "i1", qty: 1 }] },
      });
    });

    it("warns that a sale past the window needs a supervisor", async () => {
      vi.mocked(commands.exchangeLookup).mockImplementation(() =>
        ok({ ...SOURCE, days_since: 9, late: true }),
      );
      await renderTill();
      key("F12");
      const dialog = await screen.findByRole("dialog", { name: "Exchange" });
      fireEvent.change(within(dialog).getByLabelText("Receipt number of the sale"), {
        target: { value: "T01-20261105-0007" },
      });
      fireEvent.click(within(dialog).getByRole("button", { name: "Find sale" }));
      expect(await within(dialog).findByText(/More than 7 days ago/)).toBeInTheDocument();
    });

    it("never holds or recalls a cart that is mid-exchange", async () => {
      await startExchange();
      act(() => cartStore.getState().addProduct(BUCKET));
      key("F8");
      expect(await screen.findByText("Finish or cancel the exchange first.")).toBeInTheDocument();
      expect(commands.cartHold).not.toHaveBeenCalled();
      expect(cartStore.getState().lines).toHaveLength(1);
    });
  });

  describe("cart keyboard navigation and shortcuts", () => {
    it("navigates cart items with Up and Down arrows when search field is empty", async () => {
      const input = await renderTill();
      act(() => {
        cartStore.getState().addProduct(BUCKET);
        cartStore.getState().addProduct(LIME);
      });
      expect(cartStore.getState().selectedKey).toBe(cartStore.getState().lines[1]?.key);

      fireEvent.keyDown(input, { key: "ArrowUp", code: "ArrowUp" });
      expect(cartStore.getState().selectedKey).toBe(cartStore.getState().lines[0]?.key);

      fireEvent.keyDown(input, { key: "ArrowDown", code: "ArrowDown" });
      expect(cartStore.getState().selectedKey).toBe(cartStore.getState().lines[1]?.key);
    });

    it("adjusts quantity of selected item with Ctrl+ArrowRight and Ctrl+ArrowLeft", async () => {
      const input = await renderTill();
      act(() => {
        cartStore.getState().addProduct(BUCKET);
      });
      const lineKey = cartStore.getState().lines[0]?.key;
      expect(lineKey).toBeDefined();
      expect(cartStore.getState().lines[0]?.qty).toBe(1);

      fireEvent.keyDown(input, { key: "ArrowRight", code: "ArrowRight", ctrlKey: true });
      expect(cartStore.getState().lines.find((l) => l.key === lineKey)?.qty).toBe(2);

      fireEvent.keyDown(input, { key: "ArrowLeft", code: "ArrowLeft", ctrlKey: true });
      expect(cartStore.getState().lines.find((l) => l.key === lineKey)?.qty).toBe(1);

      fireEvent.keyDown(input, { key: "ArrowLeft", code: "ArrowLeft", ctrlKey: true });
      expect(cartStore.getState().lines.find((l) => l.key === lineKey)?.qty).toBe(1);
    });

    it("deletes selected cart item with Delete key when search field is empty", async () => {
      const input = await renderTill();
      act(() => {
        cartStore.getState().addProduct(BUCKET);
        cartStore.getState().addProduct(LIME);
      });
      expect(cartStore.getState().lines).toHaveLength(2);
      expect(cartStore.getState().selectedKey).toBe(cartStore.getState().lines[1]?.key);

      fireEvent.keyDown(input, { key: "Delete", code: "Delete" });
      expect(cartStore.getState().lines).toHaveLength(1);
      expect(cartStore.getState().lines[0]?.name).toBe("Blue Bucket");
    });

    it("removes line via dedicated remove button in cart row", async () => {
      await renderTill();
      act(() => {
        cartStore.getState().addProduct(BUCKET);
      });
      expect(cartStore.getState().lines).toHaveLength(1);

      const removeBtn = screen.getByRole("button", { name: `Remove ${BUCKET.name}` });
      fireEvent.click(removeBtn);
      expect(cartStore.getState().lines).toHaveLength(0);
    });

    it("opens and cancels dialog using the same function key", async () => {
      await renderTill();
      act(() => {
        cartStore.getState().addProduct(BUCKET);
      });

      // Press F1 opens Help dialog
      key("F1");
      expect(await screen.findByRole("dialog", { name: "Till shortcuts" })).toBeInTheDocument();

      // Press F1 again cancels/closes Help dialog
      key("F1");
      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Till shortcuts" })).not.toBeInTheDocument(),
      );

      // Press F9 opens Payment dialog
      key("F9");
      expect(await screen.findByRole("dialog")).toBeInTheDocument();

      // Press F9 again cancels/closes Payment dialog
      key("F9");
      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    });

    it("counts items by cart rows count (row=3 means items=3 even if qty > 1)", async () => {
      await renderTill();
      act(() => {
        cartStore.getState().addProduct(BUCKET);
        const firstKey = cartStore.getState().lines[0]?.key;
        if (firstKey) cartStore.getState().increment(firstKey, 4); // qty = 5
        cartStore.getState().addProduct(LIME);
        cartStore.getState().addProduct(MINT);
      });
      expect(screen.getByText("Items: 3")).toBeInTheDocument();
    });

    it("renders Clear cart button with red border class", async () => {
      await renderTill();
      act(() => {
        cartStore.getState().addProduct(BUCKET);
      });
      const clearBtn = screen.getByRole("button", { name: /Clear cart/i });
      expect(clearBtn).toHaveClass("border-destructive");
      expect(clearBtn).toHaveClass("text-destructive");
    });

    it("opens item discount dialog with F6 and applies single item discount (% button removed)", async () => {
      await renderTill();
      act(() => {
        cartStore.getState().addProduct(BUCKET);
      });
      // % button removed from cart row
      expect(screen.queryByRole("button", { name: /Item discount/ })).not.toBeInTheDocument();

      // Open line discount dialog with F6
      key("F6");
      const dialog = await screen.findByRole("dialog", { name: /Edit line/ });
      const discountInput = within(dialog).getByLabelText(/Line discount/);
      fireEvent.change(discountInput, { target: { value: "50" } });
      fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));

      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
      expect(cartStore.getState().lines[0]?.lineDiscount).toBe(5_000);
    });

    it("renders line total with whitespace-nowrap preventing awkward line breaks", async () => {
      await renderTill();
      act(() => {
        cartStore.getState().addProduct(BUCKET);
        const firstKey = cartStore.getState().lines[0]?.key;
        if (firstKey) cartStore.getState().increment(firstKey, 150);
      });
      const cart = screen.getByRole("list", { name: "Cart" });
      const totalEl = within(cart).getByText(/Rs 67,950.00/);
      expect(totalEl).toHaveClass("whitespace-nowrap");
    });

    it("payment dialog uses Select for method, defaults to Cash and cashier staff_no, has Pay Now button", async () => {
      await renderTill();
      act(() => {
        cartStore.getState().addProduct(BUCKET);
      });
      key("F9");
      const dialog = await screen.findByRole("dialog", { name: /Payment/ });
      // Salesperson defaults to logged-in user staff_no (1)
      const salesInput = within(dialog).getByLabelText("Salesperson no.");
      expect(salesInput).toHaveValue("1");
      // Helper text removed
      expect(within(dialog).queryByText(/Type the staff number/)).not.toBeInTheDocument();
      // Cash Received (Rs) label
      expect(within(dialog).getByLabelText(/Cash Received \(Rs\)/)).toBeInTheDocument();
      // No Exact button
      expect(within(dialog).queryByRole("button", { name: "Exact" })).not.toBeInTheDocument();
      // No + Card / + Bank transfer add buttons
      expect(within(dialog).queryByRole("group", { name: /Add payment/ })).not.toBeInTheDocument();
      // Pay Now button present
      expect(within(dialog).getByRole("button", { name: "Pay Now" })).toBeInTheDocument();
      // One tap per method, Cash selected to start
      expect(within(dialog).getByRole("radio", { name: "Cash" })).toBeChecked();
      expect(within(dialog).getByRole("radio", { name: "Card" })).not.toBeChecked();
      fireEvent.click(within(dialog).getByRole("radio", { name: "Card" }));
      expect(within(dialog).getByRole("radio", { name: "Card" })).toBeChecked();
      expect(within(dialog).getByLabelText(/Reference/)).toBeInTheDocument();
    });
  });
});
