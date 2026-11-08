import { fireEvent, screen, waitFor } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSession } from "@/app/session";
import { commands } from "@/db/bindings";
import { err, ok, renderWithProviders, session } from "@/test/render";
import { ApprovalDialog } from "./ApprovalDialog";

vi.mock("@/db/bindings", () => ({ commands: { listLoginStaff: vi.fn(), requestApproval: vi.fn() } }));

function AsCashier({ children }: { children: ReactNode }) {
  const { setSession } = useSession();
  useEffect(() => setSession(session()), [setSession]);
  return <>{children}</>;
}

beforeEach(() => {
  vi.mocked(commands.listLoginStaff).mockImplementation(() =>
    ok([
      { id: "cashier", display_name: "Nimal", role_name: "Cashier" },
      { id: "sup", display_name: "Kamal", role_name: "Supervisor" },
    ]),
  );
  vi.mocked(commands.requestApproval).mockReset();
});

function open(onApproved = vi.fn()) {
  renderWithProviders(
    <AsCashier>
      <ApprovalDialog open perm="VOID_ORDER" targetRef="order-1" onApproved={onApproved} onCancel={vi.fn()} />
    </AsCashier>,
  );
  return onApproved;
}

describe("ApprovalDialog", () => {
  it("lets a different staff member approve with their PIN and returns the single-use token", async () => {
    vi.mocked(commands.requestApproval).mockImplementation(() =>
      ok({ approval_id: "tok-1", expires_at: "2026-11-08T09:01:00.000Z" }),
    );
    const onApproved = open();
    expect(screen.getByText("Approval needed: void a sale")).toBeInTheDocument();
    await screen.findByRole("button", { name: "Kamal" });
    expect(screen.queryByRole("button", { name: "Nimal" })).not.toBeInTheDocument(); // no self-approval
    fireEvent.click(screen.getByRole("button", { name: "Kamal" }));
    for (const d of "1357") fireEvent.click(screen.getByRole("button", { name: `Digit ${d}` }));
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    await waitFor(() => expect(onApproved).toHaveBeenCalledWith("tok-1"));
    expect(commands.requestApproval).toHaveBeenCalledWith("VOID_ORDER", "order-1", "sup", "1357");
  });

  it("shows why an approval was refused", async () => {
    vi.mocked(commands.requestApproval).mockImplementation(() =>
      err({ kind: "FORBIDDEN", perm: "VOID_ORDER" }),
    );
    const onApproved = open();
    fireEvent.click(await screen.findByRole("button", { name: "Kamal" }));
    fireEvent.click(screen.getByRole("button", { name: "Digit 1" }));
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/permission to do this/);
    expect(onApproved).not.toHaveBeenCalled();
  });
});
