import { useSyncExternalStore } from "react";

const MOBILE_BREAKPOINT = 768;
const QUERY = `(max-width: ${MOBILE_BREAKPOINT - 1}px)`;

function subscribe(onChange: () => void): () => void {
  const mql = window.matchMedia(QUERY);
  mql.addEventListener("change", onChange);
  return () => mql.removeEventListener("change", onChange);
}

const isNarrow = () => window.innerWidth < MOBILE_BREAKPOINT;

/** Narrower than 768 px (the shadcn Sidebar becomes a sheet). The POS window never goes below 1024 px. */
export function useIsMobile(): boolean {
  return useSyncExternalStore(subscribe, isNarrow, () => false);
}
