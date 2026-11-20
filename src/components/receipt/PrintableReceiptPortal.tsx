import { useEffect } from "react";
import { create } from "zustand";
import type { ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { useReceiptHeader } from "@/db/hooks";
import { SinhalaReceiptTemplate } from "./SinhalaReceiptTemplate";

interface PrintStore {
  activeReceipt: ReceiptData | null;
  print: (receipt: ReceiptData) => void;
  clear: () => void;
}

export const usePrintReceiptStore = create<PrintStore>((set) => ({
  activeReceipt: null,
  print: (receipt: ReceiptData) => {
    set({ activeReceipt: receipt });
    // Allow DOM to update before calling window.print()
    setTimeout(() => {
      window.print();
    }, 80);
  },
  clear: () => set({ activeReceipt: null }),
}));

export function PrintableReceiptPortal() {
  const activeReceipt = usePrintReceiptStore((s) => s.activeReceipt);
  const clear = usePrintReceiptStore((s) => s.clear);
  const header = useReceiptHeader();

  useEffect(() => {
    const handleAfterPrint = () => {
      clear();
    };
    window.addEventListener("afterprint", handleAfterPrint);
    return () => {
      window.removeEventListener("afterprint", handleAfterPrint);
    };
  }, [clear]);

  if (!activeReceipt) return null;

  return (
    <div id="printable-receipt-portal">
      <SinhalaReceiptTemplate
        receipt={activeReceipt}
        logo={header.data?.logo}
        storeName={header.data?.store_name}
      />
    </div>
  );
}
