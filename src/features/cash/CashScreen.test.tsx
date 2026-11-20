import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import {
  commands,
  type CashSession_Serialize as CashSession,
  type Perm,
  type ZReport_Serialize as ZReport,
} from "@/db/bindings";
import { err, ok, renderWithProviders, session } from "@/test/render";
import { CashScreen, denominationTotal } from "./CashScreen";

vi.mock("@/db/bindings", () => ({
  commands: {
    currentCashSession: vi.fn(),
    openCashSession: vi.fn(),
    cashDrawerMovement: vi.fn(),
    noSale: vi.fn(),
    closeCashSession: vi.fn(),
    cashSessionReport: vi.fn(),
    cashHistory: vi.fn(),
    reprintZReport: vi.fn(),
    businessToday: vi.fn(),
    listLoginStaff: vi.fn(),
    requestApproval: vi.fn(),
  },
}));

const open: CashSession = {
  id: "cs1",
  device_id: "dev1",
  business_date: "2026-11-08",
  status: "open",
  opened_by: "cashier",
  opened_by_name: "Nimal",
  opened_at: "2026-11-08T03:30:00.000Z",
  opening_float: 500_000,
  closed_by: null,
  closed_by_name: null,
  closed_at: null,
  expected_cash: null,
  counted_cash: null,
  variance: null,
  close_note: null,
};

const z: ZReport = {
  session: {
    ...open,
    status: "closed",
    closed_by: "cashier",
    closed_by_name: "Nimal",
    closed_at: "2026-11-08T14:00:00.000Z",
  },
  generated_at: "2026-11-08T14:00:00.000Z",
  sales_count: 2,
  items_sold: 3,
  gross_sales: 85_000,
  discount_total: 0,
  net_sales: 85_000,
  sales_total: 85_000,
  payments: [{ method: "cash", count: 2, amount: 85_000 }],
  change_given: 1_000,
  overrides_count: 0,
  overrides_value: 0,
  quick_sale_lines: 0,
  quick_sale_net: 0,
  voids_count: 1,
  exchanges_count: 0,
  exchange_credit: 0,
  returned_items: 0,
  voids_value: 44_500,
  void_reasons: [{ reason: "WRONG_ITEM", count: 1, value: 44_500 }],
  voids: [
    {
      receipt_no: "T01-20261108-0003",
      value: 44_500,
      reason: "WRONG_ITEM",
      note: null,
      voided_by: "Nimal",
      approved_by: "Kamal",
      voided_at: "2026-11-08T10:00:00.000Z",
    },
  ],
  drawer: [],
  pay_ins: 0,
  pay_outs: 0,
  no_sales: 0,
  opening_float: 500_000,
  cash_sales: 85_000,
  expected_cash: 585_000,
  counted_cash: 580_000,
  variance: -5_000,
};

/** The X-report of the open shift: Rs 5,000 float + Rs 850 cash sales + Rs 100 in − Rs 25 out. */
const live: ZReport = {
  ...z,
  session: open,
  sales_count: 2,
  sales_total: 85_000,
  pay_ins: 10_000,
  pay_outs: 2_500,
  expected_cash: 592_500,
  counted_cash: null,
  variance: null,
  voids_count: 0,
  voids_value: 0,
  void_reasons: [],
  voids: [],
  drawer: [
    {
      kind: "pay_out",
      amount: 2_500,
      reason: "Tea",
      staff: "Nimal",
      approved_by: "Kamal",
      created_at: "2026-11-08T05:00:00.000Z",
    },
  ],
};

function As({ perms, children }: { perms: Perm[]; children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session({ permissions: perms })), [setSession, perms]);
  return <>{children}</>;
}

const CASHIER: Perm[] = ["SALE_CREATE", "CASH_SESSION_MANAGE"];

function renderAs(perms: Perm[] = CASHIER) {
  renderWithProviders(
    <As perms={perms}>
      <CashScreen />
    </As>,
  );
}

beforeAll(() => {
  // The virtualised table measures its scroll element; jsdom has no layout.
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 480 });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 1100 });
});

beforeEach(() => {
  vi.mocked(commands.currentCashSession).mockImplementation(() => ok(open));
  vi.mocked(commands.openCashSession).mockReset();
  vi.mocked(commands.cashDrawerMovement).mockReset();
  vi.mocked(commands.closeCashSession).mockReset();
  vi.mocked(commands.reprintZReport).mockReset();
  vi.mocked(commands.cashHistory).mockReset();
  vi.mocked(commands.cashSessionReport).mockImplementation(() => ok(live));
  vi.mocked(commands.businessToday).mockImplementation(() => ok("2026-11-20"));
  vi.mocked(commands.listLoginStaff).mockImplementation(() =>
    ok([
      { id: "cashier", display_name: "Nimal", role_name: "Cashier" },
      { id: "sup", display_name: "Kamal", role_name: "Supervisor" },
    ]),
  );
  vi.mocked(commands.requestApproval).mockImplementation(() =>
    ok({ approval_id: "tok-1", expires_at: "2026-11-08T09:01:00.000Z" }),
  );
});

describe("CashScreen", () => {
  it("opens a session with a float in cents", async () => {
    vi.mocked(commands.currentCashSession).mockImplementation(() => ok(null));
    vi.mocked(commands.openCashSession).mockImplementation(() => ok(open));
    renderAs();
    fireEvent.change(await screen.findByLabelText("Opening float (Rs)"), { target: { value: "5,000" } });
    fireEvent.click(screen.getByRole("button", { name: "Open session" }));
    await waitFor(() => expect(commands.openCashSession).toHaveBeenCalledWith({ opening_float: 500_000 }));
  });

  it("tells a viewer without CASH_SESSION_MANAGE that no session is open", async () => {
    vi.mocked(commands.currentCashSession).mockImplementation(() => ok(null));
    renderAs(["SALE_CREATE"]);
    expect(await screen.findByText(/No cash session is open/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open session" })).not.toBeInTheDocument();
  });

  it("asks a supervisor when a cashier pays out, then retries with the approval", async () => {
    vi.mocked(commands.cashDrawerMovement)
      .mockImplementationOnce(() => err({ kind: "APPROVAL_REQUIRED", perm: "CASH_PAY_IN_OUT" }))
      .mockImplementationOnce(() => ok(z));
    renderAs();
    fireEvent.click(await screen.findByRole("button", { name: "Pay out" }));
    const form = screen.getByRole("form", { name: "Pay cash out" });
    fireEvent.change(within(form).getByLabelText("Amount (Rs)"), { target: { value: "25" } });
    fireEvent.change(within(form).getByLabelText("Reason"), { target: { value: "Tea" } });
    fireEvent.click(within(form).getByRole("button", { name: "Record and open drawer" }));

    fireEvent.click(await screen.findByRole("button", { name: "Kamal" }));
    fireEvent.change(screen.getByLabelText("PIN"), { target: { value: "1357" } });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() => expect(commands.cashDrawerMovement).toHaveBeenCalledTimes(2));
    expect(commands.requestApproval).toHaveBeenCalledWith("CASH_PAY_IN_OUT", "cash_drawer", "sup", "1357");
    expect(commands.cashDrawerMovement).toHaveBeenLastCalledWith(
      { kind: "pay_out", amount: 2_500, reason: "Tea" },
      "tok-1",
    );
  });

  it("shows what should be in the drawer now, with the day's cash movements", async () => {
    renderAs();
    await waitFor(() => expect(screen.getByTestId("expected-cash")).toHaveTextContent("Rs 5,925.00"));
    expect(await screen.findByText(/Tea/)).toBeInTheDocument();
    expect(screen.getByText(/Pay out/, { selector: "li span" })).toBeInTheDocument();
  });

  it("guides the close: counts notes and coins, says short or over before confirming, then shows the Z-report", async () => {
    vi.mocked(commands.closeCashSession).mockImplementation(() => ok(z));
    renderAs();
    await screen.findByTestId("expected-cash");
    await waitFor(() => expect(commands.cashSessionReport).toHaveBeenCalled());
    fireEvent.click(await screen.findByRole("button", { name: "Close session" }));
    const form = screen.getByRole("form", { name: "Count the drawer and close" });
    fireEvent.change(within(form).getByLabelText("Rs 5,000.00"), { target: { value: "1" } });
    fireEvent.change(within(form).getByLabelText("Rs 100.00"), { target: { value: "8" } });
    expect(within(form).getByText("Rs 5,800.00")).toBeInTheDocument();
    expect(within(form).getByText("Rs 5,925.00")).toBeInTheDocument();
    expect(within(form).getByText("Short by Rs 125.00")).toBeInTheDocument();
    fireEvent.change(within(form).getByLabelText("Rs 100.00"), { target: { value: "9" } });
    fireEvent.change(within(form).getByLabelText("Rs 50.00"), { target: { value: "5" } });
    fireEvent.change(within(form).getByLabelText("Rs 20.00"), { target: { value: "1" } });
    fireEvent.change(within(form).getByLabelText("Rs 5.00"), { target: { value: "1" } });
    expect(within(form).getByText("Rs 6,175.00")).toBeInTheDocument();
    expect(within(form).getByText("Over by Rs 250.00")).toBeInTheDocument();
    fireEvent.change(within(form).getByLabelText("Rs 50.00"), { target: { value: "" } });
    fireEvent.change(within(form).getByLabelText("Rs 20.00"), { target: { value: "" } });
    fireEvent.change(within(form).getByLabelText("Rs 5.00"), { target: { value: "" } });
    fireEvent.change(within(form).getByLabelText("Rs 100.00"), { target: { value: "8" } });

    fireEvent.click(within(form).getByRole("button", { name: "Close and print Z-report" }));
    expect(commands.closeCashSession).not.toHaveBeenCalled();
    expect(within(form).getByRole("status")).toHaveTextContent(/ends voids/);
    fireEvent.click(within(form).getByRole("button", { name: "Yes, close" }));

    await waitFor(() =>
      expect(commands.closeCashSession).toHaveBeenCalledWith({
        counted_cash: 580_000,
        denominations: [
          { value: 500_000, count: 1 },
          { value: 10_000, count: 8 },
        ],
        note: null,
      }),
    );
    const report = await screen.findByRole("region", { name: /Z-report/ });
    expect(within(report).getByText("Rs 5,850.00")).toBeInTheDocument();
    expect(within(report).getByText("-Rs 50.00")).toBeInTheDocument();
    expect(within(report).getByText(/Wrong item/)).toHaveTextContent("approved by Kamal");
  });

  it("can enter a total instead of counting notes", async () => {
    vi.mocked(commands.closeCashSession).mockImplementation(() => ok(z));
    renderAs();
    fireEvent.click(await screen.findByRole("button", { name: "Close session" }));
    const form = screen.getByRole("form", { name: "Count the drawer and close" });
    fireEvent.click(within(form).getByRole("button", { name: "Enter a total instead" }));
    fireEvent.change(within(form).getByLabelText("Counted cash (Rs)"), { target: { value: "5,925" } });
    expect(await within(form).findByText("Exact. Nothing missing.")).toBeInTheDocument();
    fireEvent.click(within(form).getByRole("button", { name: "Close and print Z-report" }));
    fireEvent.click(within(form).getByRole("button", { name: "Yes, close" }));
    await waitFor(() =>
      expect(commands.closeCashSession).toHaveBeenCalledWith({
        counted_cash: 592_500,
        denominations: null,
        note: null,
      }),
    );
  });

  it("prints the Z-report again after closing", async () => {
    vi.mocked(commands.closeCashSession).mockImplementation(() => ok(z));
    vi.mocked(commands.reprintZReport).mockImplementation(() => ok(null));
    renderAs([...CASHIER, "REPRINT_RECEIPT"]);
    fireEvent.click(await screen.findByRole("button", { name: "Close session" }));
    const form = screen.getByRole("form", { name: "Count the drawer and close" });
    fireEvent.change(within(form).getByLabelText("Rs 5,000.00"), { target: { value: "1" } });
    fireEvent.click(within(form).getByRole("button", { name: "Close and print Z-report" }));
    fireEvent.click(within(form).getByRole("button", { name: "Yes, close" }));
    fireEvent.click(await screen.findByRole("button", { name: "Print again" }));
    await waitFor(() => expect(commands.reprintZReport).toHaveBeenCalledWith("cs1"));
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(await screen.findByTestId("expected-cash")).toBeInTheDocument();
  });

  it("adds up a denomination count, ignoring blanks and refusing non-numbers", () => {
    expect(denominationTotal({})).toBe(0);
    expect(denominationTotal({ 500_000: "2", 100: " 3 " })).toBe(1_000_300);
    expect(denominationTotal({ 1_000: "x" })).toBeNull();
  });
});

describe("CashScreen history", () => {
  const HISTORY = {
    rows: [
      {
        session: { ...z.session, counted_cash: 580_000, variance: -5_000 },
        device_code: "T01",
        sales_count: 2,
        sales_total: 85_000,
      },
      {
        session: { ...z.session, id: "cs0", device_id: "dev2", variance: 0, counted_cash: 585_000 },
        device_code: "T02",
        sales_count: 5,
        sales_total: 120_000,
      },
    ],
    counters: [
      { device_id: "dev1", device_code: "T01" },
      { device_id: "dev2", device_code: "T02" },
    ],
    all_counters: true,
    truncated: false,
  };
  const openHistory = async (perms?: Perm[]) => {
    renderAs(perms);
    fireEvent.mouseDown(await screen.findByRole("tab", { name: "History" }), { button: 0 });
    fireEvent.click(screen.getByRole("tab", { name: "History" }));
  };

  it("lists past shifts with the difference in words", async () => {
    vi.mocked(commands.cashHistory).mockImplementation(() => ok(HISTORY));
    await openHistory();
    expect(await screen.findByText("Short Rs 50.00")).toBeInTheDocument();
    expect(screen.getByText("Exact")).toBeInTheDocument();
    expect(commands.cashHistory).toHaveBeenCalledWith(
      { device_id: null, variance_only: false, from: "2026-10-22", to: "2026-11-20" },
      100,
    );
  });

  it("filters to shifts with a difference", async () => {
    vi.mocked(commands.cashHistory).mockImplementation(() => ok(HISTORY));
    await openHistory();
    await screen.findByText("Short Rs 50.00");
    fireEvent.click(screen.getByRole("checkbox", { name: "Only shifts with a difference" }));
    await waitFor(() =>
      expect(commands.cashHistory).toHaveBeenLastCalledWith(
        { device_id: null, variance_only: true, from: "2026-10-22", to: "2026-11-20" },
        100,
      ),
    );
  });

  it("hides the counter filter when only this counter is visible", async () => {
    vi.mocked(commands.cashHistory).mockImplementation(() =>
      ok({
        ...HISTORY,
        all_counters: false,
        counters: HISTORY.counters.slice(0, 1),
        rows: HISTORY.rows.slice(0, 1),
      }),
    );
    await openHistory();
    await screen.findByText("Short Rs 50.00");
    expect(screen.queryByLabelText("Counter")).not.toBeInTheDocument();
  });

  it("opens a shift's Z-report and prints it again", async () => {
    vi.mocked(commands.cashHistory).mockImplementation(() => ok(HISTORY));
    vi.mocked(commands.cashSessionReport).mockImplementation(() => ok(z));
    vi.mocked(commands.reprintZReport).mockImplementation(() => ok(null));
    await openHistory([...CASHIER, "REPRINT_RECEIPT"]);
    fireEvent.click(await screen.findByText("Short Rs 50.00"));
    const dialog = await screen.findByRole("dialog", { name: /Shift on 2026-11-08 · T01/ });
    expect(await within(dialog).findByText("Rs 5,850.00")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Print again" }));
    await waitFor(() => expect(commands.reprintZReport).toHaveBeenCalledWith("cs1"));
  });
});
