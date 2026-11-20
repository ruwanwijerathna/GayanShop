// Prints a receipt the till drew itself (the Sinhala template) without any dialog: the receipt is drawn off
// screen, turned into the printer's bitmap and queued like every other receipt, so the configured printer
// gets it and a stuck printer shows in the banner.
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { create } from "zustand";
import type { ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { usePrintReceiptImage } from "@/db/hooks";
import { translateError } from "@/i18n";
import { receiptImageFromElement } from "@/lib/receiptImage";
import { SinhalaReceiptTemplate } from "./SinhalaReceiptTemplate";

interface PrintStore {
  activeReceipt: ReceiptData | null;
  print: (receipt: ReceiptData) => void;
  clear: () => void;
}

export const usePrintReceiptStore = create<PrintStore>((set) => ({
  activeReceipt: null,
  print: (receipt: ReceiptData) => set({ activeReceipt: receipt }),
  clear: () => set({ activeReceipt: null }),
}));

export function PrintableReceiptPortal() {
  const { t, i18n } = useTranslation("printing");
  const activeReceipt = usePrintReceiptStore((s) => s.activeReceipt);
  const clear = usePrintReceiptStore((s) => s.clear);
  const sheet = useRef<HTMLDivElement>(null);
  const send = usePrintReceiptImage();
  const sendRef = useRef(send.mutateAsync);
  useEffect(() => {
    sendRef.current = send.mutateAsync;
  }, [send.mutateAsync]);

  useEffect(() => {
    if (!activeReceipt) return;
    let stale = false;
    void (async () => {
      try {
        await document.fonts.ready;
        const node = sheet.current;
        if (!node || stale) return;
        const image = await receiptImageFromElement(node);
        if (stale) return;
        await sendRef.current(image);
        toast.success(t("receiptImage.sent"));
      } catch (e) {
        toast.error(e instanceof CommandError ? translateError(i18n.t, e.error) : t("receiptImage.failed"));
      } finally {
        if (!stale) clear();
      }
    })();
    return () => {
      stale = true;
    };
  }, [activeReceipt, clear, i18n.t, t]);

  if (!activeReceipt) return null;

  return (
    <div aria-hidden className="pointer-events-none fixed top-0 -left-[10000px]">
      <div ref={sheet}>
        <SinhalaReceiptTemplate receipt={activeReceipt} />
      </div>
    </div>
  );
}
