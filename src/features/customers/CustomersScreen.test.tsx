import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  commands,
  type CustomerAnalytics_Serialize as CustomerAnalytics,
  type CustomerProfile_Serialize as CustomerProfile,
  type CustomerRow_Serialize as CustomerRow,
  type ReceiptData_Serialize as ReceiptData,
} from "@/db/bindings";
import { ok, renderWithProviders } from "@/test/render";
import { CustomersScreen, formatBp } from "./CustomersScreen";

vi.mock("@/db/bindings", () => ({
  commands: {
    businessToday: vi.fn(),
    customerList: vi.fn(),
    customerProfile: vi.fn(),
    customerHistory: vi.fn(),
    customerOrderReceipt: vi.fn(),
    customerAnalytics: vi.fn(),
    updateCustomer: vi.fn(),
    eraseCustomer: vi.fn(),
  },
}));

const NIMAL: CustomerRow = {
  id: "c1",
  phone_e164: "+94771234567",
  phone_display: "077 123 4567",
  display_name: "Nimal",
  visit_count: 3,
  total_spend: 125_000,
  total_net: 125_000,
  avg_basket: 41_666,
  first_visit_date: "2026-10-27",
  last_visit_date: "2026-11-08",
};

const PROFILE: CustomerProfile = {
  customer: NIMAL,
  note: "Prefers delivery",
  first_seen_at: "2026-10-27T09:00:00.000Z",
  erased_at: null,
  top_products: [
    { id: "p1", name: "Plastic chair", qty: 3, net: 75_000 },
    { id: null, name: null, qty: 1, net: 10_000 },
  ],
  top_categories: [{ id: null, name: null, qty: 4, net: 125_000 }],
};

const RECEIPT = {
  order_id: "o1",
  store_name: "GayanShop",
  receipt_no: "T01-000042",
  status: "completed",
  business_date: "2026-10-27",
  completed_at: "2026-10-27T09:00:00.000Z",
  cashier: "Kasun",
  lines: [
    {
      name: "Plastic chair",
      qty: 2,
      list_price: 25_000,
      unit_price: 25_000,
      line_gross: 50_000,
      line_discount: 0,
      line_total: 50_000,
    },
  ],
  item_count: 2,
  gross_sales: 50_000,
  discount_total: 0,
  grand_total: 50_000,
  payments: [{ method: "cash", amount: 50_000, tendered: null, reference: null }],
  change_due: 0,
  customer_masked: "07X XXX X567",
  footer: "All sales are final. No refunds or exchanges. Thank you, come again!",
} satisfies ReceiptData;

const ANALYTICS: CustomerAnalytics = {
  range: { from: "2026-11-01", to: "2026-11-30" },
  top: [
    { customer_id: "c2", display_name: "Kamala", phone_display: "071 234 5678", visits: 1, spend: 120_000 },
  ],
  trend: [{ bucket: "2026-11-02", new_customers: 1, returning_customers: 1 }],
  sales_count: 4,
  customer_orders: 3,
  attached_bp: 7_500,
};

beforeAll(() => {
  // jsdom has no layout: give the virtualised lists a viewport.
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 480 });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 800 });
});

beforeEach(() => {
  vi.mocked(commands.businessToday).mockImplementation(() => ok("2026-11-08"));
  vi.mocked(commands.customerList).mockImplementation(() => ok({ rows: [NIMAL], truncated: false }));
  vi.mocked(commands.customerProfile).mockImplementation(() => ok(PROFILE));
  vi.mocked(commands.customerHistory).mockImplementation(() =>
    ok({
      rows: [
        {
          id: "o1",
          receipt_no: "T01-000042",
          business_date: "2026-10-27",
          completed_at: "2026-10-27T09:00:00.000Z",
          status: "completed",
          item_count: 2,
          grand_total: 50_000,
        },
      ],
      next_cursor: null,
    }),
  );
  vi.mocked(commands.customerOrderReceipt).mockImplementation(() => ok(RECEIPT));
  vi.mocked(commands.customerAnalytics).mockImplementation(() => ok(ANALYTICS));
  vi.mocked(commands.updateCustomer).mockImplementation(() => ok(null));
  vi.mocked(commands.eraseCustomer).mockImplementation(() => ok(null));
});

async function openNimal() {
  renderWithProviders(<CustomersScreen />);
  const grid = await screen.findByRole("grid", { name: "Customers" });
  fireEvent.click(await within(grid).findByText("Nimal"));
  return screen.findByRole("region", { name: "Customer profile" });
}

describe("CustomersScreen", () => {
  it("lists customers by spend and re-queries on search and sort", async () => {
    renderWithProviders(<CustomersScreen />);
    const grid = await screen.findByRole("grid", { name: "Customers" });
    expect(await within(grid).findByText("077 123 4567")).toBeInTheDocument();
    expect(within(grid).getByText("Rs 1,250.00")).toBeInTheDocument();
    expect(commands.customerList).toHaveBeenCalledWith(null, "spend");
    fireEvent.change(screen.getByLabelText("Search by phone or name"), { target: { value: "0771" } });
    await waitFor(() => expect(commands.customerList).toHaveBeenLastCalledWith("0771", "spend"));
    fireEvent.click(screen.getByRole("button", { name: "Last visit" }));
    await waitFor(() => expect(commands.customerList).toHaveBeenLastCalledWith("0771", "last_visit"));
  });

  it("opens a profile with totals, top products and history down to the masked receipt", async () => {
    const profile = await openNimal();
    expect(await within(profile).findByText("Rs 416.66")).toBeInTheDocument();
    expect(within(profile).getByText("Plastic chair")).toBeInTheDocument();
    expect(within(profile).getByText("Quick sales")).toBeInTheDocument();
    expect(within(profile).getByText("Uncategorised")).toBeInTheDocument();
    const history = within(profile).getByRole("grid", { name: "Purchase history" });
    fireEvent.click(await within(history).findByText("T01-000042"));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/07X XXX X567/)).toBeInTheDocument();
    expect(within(dialog).getAllByText("Rs 500.00").length).toBeGreaterThan(0);
    expect(commands.customerOrderReceipt).toHaveBeenCalledWith("c1", "o1");
  });

  it("saves the name and note", async () => {
    const profile = await openNimal();
    const form = await within(profile).findByRole("form", { name: "Edit customer" });
    expect(within(form).getByLabelText("Note (optional)")).toHaveValue("Prefers delivery");
    fireEvent.change(within(form).getByLabelText("Name (optional)"), { target: { value: "Nimal Perera" } });
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(commands.updateCustomer).toHaveBeenCalledWith("c1", {
        display_name: "Nimal Perera",
        note: "Prefers delivery",
      }),
    );
  });

  it("erases personal data only after the explained confirmation", async () => {
    const profile = await openNimal();
    fireEvent.click(await within(profile).findByRole("button", { name: "Erase personal data" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/old visits stay anonymous/)).toBeInTheDocument();
    expect(commands.eraseCustomer).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Erase" }));
    await waitFor(() => expect(commands.eraseCustomer).toHaveBeenCalledWith("c1"));
  });

  it("shows analytics for this month and switches buckets and periods", async () => {
    renderWithProviders(<CustomersScreen />);
    fireEvent.click(await screen.findByRole("tab", { name: "Analytics" }));
    expect(await screen.findByText("75.0% of sales had a customer (3 of 4).")).toBeInTheDocument();
    expect(screen.getByText("Kamala")).toBeInTheDocument();
    expect(commands.customerAnalytics).toHaveBeenCalledWith(
      { kind: "month", year: 2026, month: 11 },
      "week",
      10,
    );
    fireEvent.click(screen.getByRole("button", { name: "By month" }));
    await waitFor(() =>
      expect(commands.customerAnalytics).toHaveBeenLastCalledWith(
        { kind: "month", year: 2026, month: 11 },
        "month",
        10,
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "All time" }));
    await waitFor(() =>
      expect(commands.customerAnalytics).toHaveBeenLastCalledWith({ kind: "all_time" }, "month", 10),
    );
  });

  it("formats basis points as a percentage", () => {
    expect(formatBp(7_500)).toBe("75.0%");
    expect(formatBp(3_333)).toBe("33.3%");
    expect(formatBp(0)).toBe("0.0%");
  });
});
