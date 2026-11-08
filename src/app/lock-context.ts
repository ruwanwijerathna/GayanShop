import { createContext, useContext } from "react";

/** Lets the logged-in shell tell <App> that a lock/logout was explicit (so it isn't shown as an idle lock). */
export const ExplicitLockContext = createContext<() => void>(() => undefined);

export function useMarkExplicitLock(): () => void {
  return useContext(ExplicitLockContext);
}
