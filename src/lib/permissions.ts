import type { Perm, SessionInfo } from "@/db/bindings";

/** UI-side check for HIDING controls only — Rust enforces every permission (AGENT.md §2). */
export function hasPerm(session: SessionInfo | null, perm: Perm): boolean {
  return session?.permissions.includes(perm) ?? false;
}
