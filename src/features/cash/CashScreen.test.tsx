import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
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

beforeEach(() => {
  vi.mocked(commands.currentCashSession).mockImplementation(() => ok(open));
  vi.mocked(commands.openCashSession).mockReset();
  vi.mocked(commands.cashDrawerMovement).mockReset();
  vi.mocked(commands.closeCashSession).mockReset();
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

  it("closes by denomination after confirmation and shows the Z-report", async () => {
    vi.mocked(commands.closeCashSession).mockImplementation(() => ok(z));
    renderAs();
    fireEvent.click(await screen.findByRole("button", { name: "Close session" }));
    const form = screen.getByRole("form", { name: "Count the drawer and close" });
    fireEvent.click(within(form).getByRole("checkbox", { name: "Count by note and coin" }));
    fireEvent.change(within(form).getByLabelText("Rs 5,000.00"), { target: { value: "1" } });
    fireEvent.change(within(form).getByLabelText("Rs 100.00"), { target: { value: "8" } });
    expect(within(form).getByText("Rs 5,800.00")).toBeInTheDocument();

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

  it("adds up a denomination count, ignoring blanks and refusing non-numbers", () => {
    expect(denominationTotal({})).toBe(0);
    expect(denominationTotal({ 500_000: "2", 100: " 3 " })).toBe(1_000_300);
    expect(denominationTotal({ 1_000: "x" })).toBeNull();
  });
});
