// Prints a receipt or document the till drew itself (the Sinhala FM templates) without any dialog:
// the document is drawn off-screen, turned into the printer's bitmap and queued like every other
// print job, so the configured printer gets it and a stuck printer shows in the banner.
import { useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { create } from "zustand";
import type { ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { usePrinterProfile, usePrintReceiptImage } from "@/db/hooks";
import { translateError } from "@/i18n";
import { receiptImageFromElement } from "@/lib/receiptImage";
import {
  CashMovementTemplate,
  EstimateTemplate,
  ReturnExchangeTemplate,
  SaleReceiptTemplate,
  ZReportTemplate,
} from "./templates";
import type { PrintableItem } from "./types";

export type { PrintableItem };

interface PrintStore {
  activeReceipt: ReceiptData | null;
  activeItem: PrintableItem | null;
  /** A reprint of a real bill is audited against it; a sample (Settings test receipt) is not. */
  reprint: boolean;
  print: (receipt: ReceiptData, options?: { reprint?: boolean }) => void;
  printTemplate: (item: PrintableItem, options?: { reprint?: boolean }) => void;
  clear: () => void;
}

export const usePrintReceiptStore = create<PrintStore>((set) => ({
  activeReceipt: null,
  activeItem: null,
  reprint: false,
  print: (receipt: ReceiptData, options?: { reprint?: boolean }) => {
    const isReturnExchange = (receipt.returns && receipt.returns.length > 0) || receipt.exchange_credit > 0;
    const item: PrintableItem = isReturnExchange
      ? { type: "return_exchange", data: receipt }
      : { type: "sale", data: receipt };
    set({
      activeReceipt: receipt,
      activeItem: item,
      reprint: options?.reprint ?? false,
    });
  },
  printTemplate: (item: PrintableItem, options?: { reprint?: boolean }) => {
    const receipt = item.type === "sale" || item.type === "return_exchange" ? item.data : null;
    set({
      activeReceipt: receipt,
      activeItem: item,
      reprint: options?.reprint ?? false,
    });
  },
  clear: () => set({ activeReceipt: null, activeItem: null, reprint: false }),
}));

function renderTemplate(item: PrintableItem, defaultQr = false) {
  const qr = ("qr" in item ? item.qr : undefined) ?? defaultQr;
  switch (item.type) {
    case "sale":
      return (
        <SaleReceiptTemplate
          receipt={item.data}
          logo={item.logo}
          logoDataUrl={item.logoDataUrl}
          storeName={item.storeName}
          qr={qr}
        />
      );
    case "return_exchange":
      return (
        <ReturnExchangeTemplate
          receipt={item.data}
          logo={item.logo}
          logoDataUrl={item.logoDataUrl}
          storeName={item.storeName}
          qr={qr}
        />
      );
    case "z_report":
      return (
        <ZReportTemplate
          report={item.data}
          logo={item.logo}
          logoDataUrl={item.logoDataUrl}
          storeName={item.storeName}
        />
      );
    case "cash_movement":
      return (
        <CashMovementTemplate
          data={item.data}
          logo={item.logo}
          logoDataUrl={item.logoDataUrl}
          storeName={item.storeName}
        />
      );
    case "estimate":
      return (
        <EstimateTemplate
          data={item.data}
          logo={item.logo}
          logoDataUrl={item.logoDataUrl}
          storeName={item.storeName}
        />
      );
  }
}

export function PrintableReceiptPortal() {
  const { t, i18n } = useTranslation("printing");
  const profile = usePrinterProfile();
  const activeReceipt = usePrintReceiptStore((s) => s.activeReceipt);
  const activeItem = usePrintReceiptStore((s) => s.activeItem);
  const reprint = usePrintReceiptStore((s) => s.reprint);
  const clear = usePrintReceiptStore((s) => s.clear);
  const sheet = useRef<HTMLDivElement>(null);
  const send = usePrintReceiptImage();
  const sendRef = useRef(send.mutateAsync);

  useEffect(() => {
    sendRef.current = send.mutateAsync;
  }, [send.mutateAsync]);

  const itemToPrint: PrintableItem | null = useMemo(
    () => activeItem ?? (activeReceipt ? { type: "sale", data: activeReceipt } : null),
    [activeItem, activeReceipt],
  );

  useEffect(() => {
    if (!itemToPrint) return;
    let stale = false;
    void (async () => {
      try {
        await document.fonts.ready;
        const node = sheet.current;
        if (!node || stale) return;
        const image = await receiptImageFromElement(node);
        if (stale) return;
        await sendRef.current({
          image,
          orderId: reprint && activeReceipt ? activeReceipt.order_id : null,
        });
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
  }, [itemToPrint, activeReceipt, reprint, clear, i18n.t, t]);

  if (!itemToPrint) return null;

  return (
    <div aria-hidden className="pointer-events-none fixed top-0 -left-[10000px]">
      <div ref={sheet}>{renderTemplate(itemToPrint, profile.data?.qr)}</div>
    </div>
  );
}
