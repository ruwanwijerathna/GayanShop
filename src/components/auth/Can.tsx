import type { ReactNode } from "react";
import { useSession } from "@/app/session";
import type { Perm } from "@/db/bindings";
import { hasPerm } from "@/lib/permissions";

/** Render `children` only if the logged-in staff member holds `perm` (hides only; Rust enforces). */
export function Can({
  perm,
  children,
  fallback = null,
}: {
  perm: Perm;
  children: ReactNode;
  fallback?: ReactNode;
}) {
  const { session } = useSession();
  return <>{hasPerm(session, perm) ? children : fallback}</>;
}
