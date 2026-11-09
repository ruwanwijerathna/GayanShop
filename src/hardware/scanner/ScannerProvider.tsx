// The app-wide scanner (doc §7.1). Mounted once at the root: the capture listener must be registered on
// window in the capture phase, ahead of every input, dialog and shortcut handler.
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { ScannerCapture } from "./capture";
import { ScanQueue, type ScanHandler } from "./queue";

export interface Scanner {
  /** Route scans to `handler` (the screen in front); returns the unsubscribe. */
  subscribe: (handler: ScanHandler) => () => void;
  /** True mid-scan: keyboard shortcuts must not fire. */
  isCapturing: () => boolean;
}

const ScannerContext = createContext<Scanner | null>(null);

function createScanner() {
  // The newest subscriber is the screen in front; earlier ones get scans back when it unmounts.
  let handlers: ScanHandler[] = [];
  const queue = new ScanQueue((code) => handlers.at(-1)?.(code));
  const capture = new ScannerCapture({ onScan: (code) => queue.push(code) });
  const api: Scanner = {
    subscribe: (handler) => {
      handlers = [...handlers, handler];
      return () => {
        handlers = handlers.filter((h) => h !== handler);
      };
    },
    isCapturing: () => capture.capturing(),
  };
  return { api, capture };
}

export function ScannerProvider({ children }: { children: ReactNode }) {
  const [scanner] = useState(createScanner);

  useEffect(() => {
    const { capture } = scanner;
    window.addEventListener("keydown", capture.handleKeyDown, { capture: true });
    return () => {
      window.removeEventListener("keydown", capture.handleKeyDown, { capture: true });
      capture.dispose();
    };
  }, [scanner]);

  return <ScannerContext.Provider value={scanner.api}>{children}</ScannerContext.Provider>;
}

/** The scanner, or a no-op one outside the provider (isolated component tests). */
export function useScanner(): Scanner {
  return useContext(ScannerContext) ?? NO_SCANNER;
}

const NO_SCANNER: Scanner = { subscribe: () => () => undefined, isCapturing: () => false };

/** Receive scans while the calling component is mounted. */
export function useScanHandler(handler: ScanHandler) {
  const { subscribe } = useScanner();
  const latest = useRef(handler);
  useEffect(() => {
    latest.current = handler;
  });
  useEffect(() => subscribe((code) => latest.current(code)), [subscribe]);
}
