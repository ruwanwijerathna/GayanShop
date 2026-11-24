// The app-wide scanner (doc §7.1). Mounted once at the root: the capture listener must be registered on
// window in the capture phase, ahead of every input, dialog and shortcut handler.
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { isBillNumber } from "@/lib/receipt";
import { ScannerCapture } from "./capture";
import { ScanQueue, type ScanHandler } from "./queue";

export interface SubscribeOptions {
  /** The screen deals with scanned bill numbers itself (the till opens the exchange). */
  handlesBills?: boolean;
}

export interface Scanner {
  /** Route scans to `handler` (the screen in front); returns the unsubscribe. */
  subscribe: (handler: ScanHandler, options?: SubscribeOptions) => () => void;
  /** Where a scanned bill number goes when the screen in front doesn't handle bills; returns the unsubscribe. */
  setBillFallback: (handler: ScanHandler) => () => void;
  /** True mid-scan: keyboard shortcuts must not fire. */
  isCapturing: () => boolean;
}

const ScannerContext = createContext<Scanner | null>(null);

function createScanner() {
  // The newest subscriber is the screen in front; earlier ones get scans back when it unmounts.
  let handlers: { handler: ScanHandler; handlesBills: boolean }[] = [];
  let billFallback: ScanHandler | null = null;
  const queue = new ScanQueue((code) => {
    const front = handlers.at(-1);
    if (billFallback && !front?.handlesBills && isBillNumber(code))
      return billFallback(code.trim().toUpperCase());
    return front?.handler(code);
  });
  const capture = new ScannerCapture({ onScan: (code) => queue.push(code) });
  const api: Scanner = {
    subscribe: (handler, options) => {
      const entry = { handler, handlesBills: options?.handlesBills ?? false };
      handlers = [...handlers, entry];
      return () => {
        handlers = handlers.filter((h) => h !== entry);
      };
    },
    setBillFallback: (handler) => {
      billFallback = handler;
      return () => {
        if (billFallback === handler) billFallback = null;
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

const NO_SCANNER: Scanner = {
  subscribe: () => () => undefined,
  setBillFallback: () => () => undefined,
  isCapturing: () => false,
};

/** Receive scans while the calling component is mounted. */
export function useScanHandler(handler: ScanHandler, options?: SubscribeOptions) {
  const { subscribe } = useScanner();
  const handlesBills = options?.handlesBills ?? false;
  const latest = useRef(handler);
  useEffect(() => {
    latest.current = handler;
  });
  useEffect(() => subscribe((code) => latest.current(code), { handlesBills }), [subscribe, handlesBills]);
}

/** Where scanned bill numbers go on every screen that doesn't handle them itself. */
export function useBillScanFallback(handler: ScanHandler) {
  const { setBillFallback } = useScanner();
  const latest = useRef(handler);
  useEffect(() => {
    latest.current = handler;
  });
  useEffect(() => setBillFallback((code) => latest.current(code)), [setBillFallback]);
}
