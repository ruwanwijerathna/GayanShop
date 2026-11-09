import { act, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands, type SyncStatus_Serialize as SyncStatus } from "@/db/bindings";
import { ok, renderWithProviders } from "@/test/render";
import { SyncStatusPill } from "./SyncStatusPill";

vi.mock("@/db/bindings", () => ({ commands: { syncStatus: vi.fn() } }));
vi.mock("@tanstack/react-router", () => ({
  Link: ({
    children,
    className,
    "aria-label": label,
  }: {
    children: ReactNode;
    className: string;
    "aria-label": string;
  }) => (
    <a href="#sync" className={className} aria-label={label}>
      {children}
    </a>
  ),
}));

// The worker's `sync://status` events, delivered by hand.
const handlers: ((e: { payload: unknown }) => void)[] = [];
vi.mock("@tauri-apps/api/event", () => ({
  listen: (_name: string, cb: (e: { payload: unknown }) => void) => {
    handlers.push(cb);
    return Promise.resolve(() => undefined);
  },
}));

function status(overrides: Partial<SyncStatus> = {}): SyncStatus {
  return {
    state: "synced",
    pending: 0,
    dead: 0,
    last_success_at: "2026-11-09T05:00:00.000Z",
    last_error: null,
    chain_broken: [],
    ...overrides,
  };
}

beforeEach(() => {
  handlers.length = 0;
  vi.mocked(commands.syncStatus).mockReset();
});

describe("SyncStatusPill", () => {
  it("shows the worker's status and follows sync://status", async () => {
    vi.mocked(commands.syncStatus).mockImplementation(() => ok(status({ state: "pending", pending: 3 })));
    renderWithProviders(<SyncStatusPill />);
    const pill = await screen.findByRole("link", { name: /Pending 3/ });
    expect(pill).toHaveClass("bg-warning");
    // The network drops: the worker reports offline with the sales still queued.
    act(() => handlers.forEach((h) => h({ payload: status({ state: "offline", pending: 5 }) })));
    expect(await screen.findByRole("link", { name: /Offline · 5 queued/ })).toHaveClass("bg-destructive");
    act(() => handlers.forEach((h) => h({ payload: status() })));
    expect(
      await screen.findByRole("link", { name: "Cloud sync: Synced. Open Settings → Sync." }),
    ).toHaveClass("bg-success-surface");
  });

  it("says when this counter isn't registered yet", async () => {
    vi.mocked(commands.syncStatus).mockImplementation(() =>
      ok(status({ state: "unregistered", last_success_at: null })),
    );
    renderWithProviders(<SyncStatusPill />);
    expect(await screen.findByText("Not syncing")).toBeInTheDocument();
  });
});
