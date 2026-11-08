import { createContext, useContext, useMemo, useState, type ReactNode } from "react";
import type { SessionInfo } from "@/db/bindings";

interface SessionState {
  session: SessionInfo | null;
  setSession: (session: SessionInfo | null) => void;
}

const SessionContext = createContext<SessionState | null>(null);

/** Read-only mirror of the Rust session, used to hide controls. Never trusted for enforcement. */
export function SessionProvider({
  initial = null,
  children,
}: {
  initial?: SessionInfo | null;
  children: ReactNode;
}) {
  const [session, setSession] = useState<SessionInfo | null>(initial);
  const value = useMemo(() => ({ session, setSession }), [session]);
  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}

export function useSession(): SessionState {
  const ctx = useContext(SessionContext);
  if (!ctx) throw new Error("useSession must be used inside <SessionProvider>");
  return ctx;
}
