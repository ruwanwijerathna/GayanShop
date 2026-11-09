import type { ReactNode } from "react";
import { useSession } from "@/app/session";
import type { Perm } from "@/db/bindings";
import { hasPerm } from "@/lib/permissions";

/** Render `children` only if the logged-in staff member holds `perm` (any of them, for a list). Hides only;
 * Rust enforces. */
export function Can({
  perm,
  children,
  fallback = null,
}: {
  perm: Perm | Perm[];
  children: ReactNode;
  fallback?: ReactNode;
}) {
  const { session } = useSession();
  const allowed = Array.isArray(perm) ? perm.some((p) => hasPerm(session, p)) : hasPerm(session, perm);
  return <>{allowed ? children : fallback}</>;
}
