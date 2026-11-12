import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  commands,
  type CashSession_Serialize as CashSession,
  type ExpenseCategory,
  type ExpenseList_Serialize as ExpenseList,
  type ExpenseRow_Serialize as ExpenseRow,
} from "@/db/bindings";
import { chooseOption, optionNames, err, ok, renderWithProviders } from "@/test/render";
import { ExpensesScreen } from "./ExpensesScreen";

vi.mock("@/db/bindings", () => ({
  commands: {
    businessToday: vi.fn(),
    currentCashSession: vi.fn(),
    expenseCategories: vi.fn(),
    expenseList: vi.fn(),
    saveExpenseCategory: vi.fn(),
    recordExpense: vi.fn(),
    updateExpense: vi.fn(),
    deleteExpense: vi.fn(),
    listLoginStaff: vi.fn(),
    requestApproval: vi.fn(),
  },
}));

const CATEGORIES: ExpenseCategory[] = [
  { id: "rent", name: "Rent", is_active: true, used: 1 },
  { id: "tea", name: "Tea & sundries", is_active: true, used: 2 },
  { id: "old", name: "Old", is_active: false, used: 0 },
];

const TEA: ExpenseRow = {
  id: "e1",
  category_id: "tea",
  category_name: "Tea & sundries",
  amount: 75_000,
  business_date: "2026-11-08",
  payment_method: "cash",
  paid_from_drawer: true,
  cash_session_id: "s1",
  payee: "CEB",
  note: null,
  recorded_by_name: "Owner",
  created_at: "2026-11-08T09:00:00.000Z",
};

const LIST: ExpenseList = {
  range: { from: "2026-11-01", to: "2026-11-30" },
  rows: [TEA],
  by_category: [{ category_id: "tea", name: "Tea & sundries", amount: 75_000, count: 1 }],
  total: 75_000,
  from_drawer: 75_000,
};

const SESSION = { id: "s1", status: "open" } as unknown as CashSession;

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 480 });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 900 });
});

beforeEach(() => {
  vi.mocked(commands.businessToday).mockImplementation(() => ok("2026-11-08"));
  vi.mocked(commands.currentCashSession).mockImplementation(() => ok(SESSION));
  vi.mocked(commands.expenseCategories).mockImplementation(() => ok(CATEGORIES));
  vi.mocked(commands.expenseList).mockImplementation(() => ok(LIST));
  vi.mocked(commands.recordExpense).mockImplementation(() => ok(TEA));
  vi.mocked(commands.updateExpense).mockImplementation(() => ok(TEA));
  vi.mocked(commands.deleteExpense).mockImplementation(() => ok(null));
  vi.mocked(commands.saveExpenseCategory).mockImplementation(() => ok("new"));
  vi.mocked(commands.listLoginStaff).mockImplementation(() => ok([]));
});

async function openAdd() {
  const add = await screen.findByRole("button", { name: "Add expense" });
  await waitFor(() => expect(add).toBeEnabled());
  fireEvent.click(add);
}

describe("ExpensesScreen", () => {
  it("lists this month's expenses with totals by category", async () => {
    renderWithProviders(<ExpensesScreen />);
    const grid = await screen.findByRole("grid", { name: "Expenses" });
    expect(await within(grid).findByText("Cash from the drawer")).toBeInTheDocument();
    const totals = screen.getByRole("region", { name: "Totals" });
    expect(within(totals).getAllByText("Rs 750.00").length).toBe(3);
    expect(commands.expenseList).toHaveBeenCalledWith({ kind: "month", year: 2026, month: 11 }, null);
    chooseOption(screen.getByLabelText("Category"), "Rent");
    await waitFor(() =>
      expect(commands.expenseList).toHaveBeenLastCalledWith({ kind: "month", year: 2026, month: 11 }, "rent"),
    );
  });

  it("records a drawer-paid expense as cash, today", async () => {
    renderWithProviders(<ExpensesScreen />);
    await openAdd();
    const form = await screen.findByRole("form", { name: "Add expense" });
    expect(optionNames(within(form).getByLabelText("Category"))).toEqual(["Rent", "Tea & sundries"]);
    chooseOption(within(form).getByLabelText("Category"), "Tea & sundries");
    fireEvent.change(within(form).getByLabelText("Amount (Rs)"), { target: { value: "750" } });
    chooseOption(within(form).getByLabelText("Paid by"), "Card");
    fireEvent.click(within(form).getByLabelText(/cash from this counter's drawer/));
    expect(within(form).getByLabelText("Paid by")).toBeDisabled();
    fireEvent.change(within(form).getByLabelText("Paid to (optional)"), { target: { value: "CEB" } });
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(commands.recordExpense).toHaveBeenCalledWith(
        {
          category_id: "tea",
          amount: 75_000,
          business_date: "2026-11-08",
          payment_method: "cash",
          paid_from_drawer: true,
          payee: "CEB",
          note: "",
        },
        null,
      ),
    );
  });

  it("asks a supervisor when the drawer needs CASH_PAY_IN_OUT", async () => {
    vi.mocked(commands.recordExpense).mockImplementationOnce(() =>
      err({ kind: "APPROVAL_REQUIRED", perm: "CASH_PAY_IN_OUT" }),
    );
    renderWithProviders(<ExpensesScreen />);
    await openAdd();
    const form = await screen.findByRole("form", { name: "Add expense" });
    fireEvent.change(within(form).getByLabelText("Amount (Rs)"), { target: { value: "100" } });
    fireEvent.click(within(form).getByLabelText(/cash from this counter's drawer/));
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/supervisor/i)).toBeInTheDocument();
  });

  it("can't pay from the drawer without an open shift", async () => {
    vi.mocked(commands.currentCashSession).mockImplementation(() => ok(null));
    renderWithProviders(<ExpensesScreen />);
    await openAdd();
    const form = await screen.findByRole("form", { name: "Add expense" });
    expect(await within(form).findByLabelText(/needs an open shift/)).toBeDisabled();
  });

  it("opens a drawer expense for editing with its cash facts locked", async () => {
    renderWithProviders(<ExpensesScreen />);
    const grid = await screen.findByRole("grid", { name: "Expenses" });
    fireEvent.click(await within(grid).findByText("CEB"));
    const form = await screen.findByRole("form", { name: "Edit expense" });
    expect(within(form).getByLabelText("Amount (Rs)")).toBeDisabled();
    expect(within(form).getByText(/amount, date and method are fixed/)).toBeInTheDocument();
    chooseOption(within(form).getByLabelText("Category"), "Rent");
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(commands.updateExpense).toHaveBeenCalledWith(
        "e1",
        expect.objectContaining({ category_id: "rent", amount: 75_000 }),
      ),
    );
  });

  it("deletes after explaining the cash goes back", async () => {
    renderWithProviders(<ExpensesScreen />);
    const grid = await screen.findByRole("grid", { name: "Expenses" });
    fireEvent.click(await within(grid).findByRole("button", { name: "Delete expense" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/cash goes back into the drawer/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(commands.deleteExpense).toHaveBeenCalledWith("e1", null));
  });

  it("adds and retires categories", async () => {
    renderWithProviders(<ExpensesScreen />);
    fireEvent.click(await screen.findByRole("button", { name: "Categories" }));
    const section = await screen.findByRole("region", { name: "Categories" });
    fireEvent.change(within(section).getByLabelText("New category"), { target: { value: "Security" } });
    fireEvent.click(within(section).getByRole("button", { name: "Add" }));
    await waitFor(() =>
      expect(commands.saveExpenseCategory).toHaveBeenCalledWith(null, { name: "Security", is_active: true }),
    );
    const rent = within(section).getByDisplayValue("Rent").closest("li");
    expect(rent).not.toBeNull();
    if (rent) fireEvent.click(within(rent).getByRole("button", { name: "Retire" }));
    await waitFor(() =>
      expect(commands.saveExpenseCategory).toHaveBeenLastCalledWith("rent", {
        name: "Rent",
        is_active: false,
      }),
    );
    expect(within(section).getByRole("button", { name: "Restore" })).toBeInTheDocument();
  });
});
