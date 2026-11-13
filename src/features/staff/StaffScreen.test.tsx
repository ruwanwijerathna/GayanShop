import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, type Role, type StaffMember_Serialize as StaffMember } from "@/db/bindings";
import { err, ok, renderWithProviders } from "@/test/render";
import { parseLimit, StaffScreen } from "./StaffScreen";

vi.mock("@/db/bindings", () => ({
  commands: {
    listStaff: vi.fn(),
    listRoles: vi.fn(),
    createStaff: vi.fn(),
    updateStaff: vi.fn(),
    resetStaffPin: vi.fn(),
    listLoginStaff: vi.fn(),
  },
}));

const ROLES: Role[] = [
  {
    id: "r_owner",
    name: "Owner",
    is_system: true,
    permissions: [],
    max_discount_bp: 10_000,
    max_override_bp: 10_000,
    min_pin_len: 6,
  },
  {
    id: "r_cash",
    name: "Cashier",
    is_system: true,
    permissions: ["SALE_CREATE", "QUICK_SALE"],
    max_discount_bp: 500,
    max_override_bp: 0,
    min_pin_len: 4,
  },
];

const member = (over: Partial<StaffMember>): StaffMember => ({
  id: "s1",
  display_name: "Gayan",
  role_id: "r_owner",
  role_name: "Owner",
  is_owner: true,
  is_active: true,
  ui_language: "en",
  grants: [],
  denies: [],
  max_discount_bp: null,
  effective: [],
  last_owner: true,
  ...over,
});

const KASUN = member({
  id: "s2",
  display_name: "Kasun",
  role_id: "r_cash",
  role_name: "Cashier",
  is_owner: false,
  last_owner: false,
  ui_language: "si",
});

beforeEach(() => {
  vi.mocked(commands.listStaff).mockImplementation(() => ok([member({}), KASUN]));
  vi.mocked(commands.listRoles).mockImplementation(() => ok(ROLES));
  vi.mocked(commands.createStaff).mockImplementation(() => ok(KASUN));
  vi.mocked(commands.updateStaff).mockImplementation(() => ok(KASUN));
  vi.mocked(commands.resetStaffPin).mockImplementation(() => ok(null));
  vi.mocked(commands.listLoginStaff).mockImplementation(() => ok([]));
});

describe("StaffScreen", () => {
  it("lists staff with role, language and status", async () => {
    renderWithProviders(<StaffScreen />);
    const row = (await screen.findByText("Kasun")).closest("tr");
    expect(row).not.toBeNull();
    if (row) expect(within(row).getByText("Sinhala")).toBeInTheDocument();
  });

  it("adds a cashier with a PIN and confirmed PIN", async () => {
    renderWithProviders(<StaffScreen />);
    const add = await screen.findByRole("button", { name: "Add staff" });
    await waitFor(() => expect(add).toBeEnabled());
    fireEvent.click(add);
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Name"), { target: { value: "Nimali" } });
    fireEvent.change(within(dialog).getByLabelText("PIN (4–8 digits)"), { target: { value: "48a26" } });
    fireEvent.change(within(dialog).getByLabelText("Confirm PIN"), { target: { value: "4826" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Add" }));
    await waitFor(() =>
      expect(commands.createStaff).toHaveBeenCalledWith({
        staff: {
          display_name: "Nimali",
          role_id: "r_cash",
          ui_language: "en",
          grants: [],
          denies: [],
          max_discount_bp: null,
          is_active: true,
        },
        pin: "4826",
      }),
    );
  });

  it("locks the role and active switch of the last Owner", async () => {
    renderWithProviders(<StaffScreen />);
    fireEvent.click(await screen.findByRole("button", { name: "Edit Gayan" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByLabelText("Role")).toBeDisabled();
    expect(within(dialog).getByLabelText("Can log in")).toBeDisabled();
    expect(within(dialog).getByText(/only active Owner/)).toBeInTheDocument();
  });

  it("shows Rust's owner-invariant refusal", async () => {
    vi.mocked(commands.updateStaff).mockImplementation(() =>
      err({ kind: "VALIDATION", code: "OWNER_ROLE_OWNER_ONLY", detail: "" }),
    );
    renderWithProviders(<StaffScreen />);
    fireEvent.click(await screen.findByRole("button", { name: "Edit Kasun" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("Only an Owner");
  });

  it("resets a PIN", async () => {
    renderWithProviders(<StaffScreen />);
    fireEvent.click(await screen.findByRole("button", { name: "Edit Kasun" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Reset PIN" }));
    fireEvent.change(within(dialog).getByLabelText("New PIN (4–8 digits)"), { target: { value: "9753" } });
    fireEvent.change(within(dialog).getByLabelText("Confirm new PIN"), { target: { value: "9753" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(commands.resetStaffPin).toHaveBeenCalledWith("s2", "9753"));
  });

  it("filters staff by search query", async () => {
    renderWithProviders(<StaffScreen />);
    expect(await screen.findByText("Gayan")).toBeInTheDocument();
    expect(screen.getByText("Kasun")).toBeInTheDocument();

    const searchInput = screen.getByLabelText("Search by name or role");
    fireEvent.change(searchInput, { target: { value: "Kasun" } });

    expect(screen.getByText("Kasun")).toBeInTheDocument();
    expect(screen.queryByText("Gayan")).not.toBeInTheDocument();
  });

  it("switches to roles tab and displays permission matrix", async () => {
    renderWithProviders(<StaffScreen />);
    const rolesTab = await screen.findByRole("tab", { name: "Roles & permissions" });
    fireEvent.click(rolesTab);

    expect(await screen.findByText("Permission matrix")).toBeInTheDocument();
    expect(screen.getByText("Sales & Till")).toBeInTheDocument();
    expect(screen.getByText("Cash Management")).toBeInTheDocument();
    expect(screen.getByText("Full access")).toBeInTheDocument();
  });

  it("parses personal discount limits as percentages", () => {
    expect(parseLimit("")).toBeNull();
    expect(parseLimit("5")).toBe(500);
    expect(parseLimit("12.5")).toBe(1_250);
    expect(parseLimit("101")).toBeUndefined();
    expect(parseLimit("x")).toBeUndefined();
  });
});
