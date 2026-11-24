// Prints a receipt or document the till drew itself (the Sinhala FM templates) without any dialog:
// the document is drawn off-screen, turned into the printer's bitmap and queued like every other
// print job, so the configured printer gets it and a stuck printer shows in the banner.
import { useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { create } from "zustand";
import type { Logo, PaperWidth, ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { CommandError } from "@/db/client";
import { usePrinterProfile, usePrintReceiptImage, useReceiptHeader } from "@/db/hooks";
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

function renderTemplate(
  item: PrintableItem,
  defaultQr = false,
  paperWidth?: PaperWidth,
  headerData?: { logo?: Logo | null; store_name?: string },
) {
  const qr = ("qr" in item ? item.qr : undefined) ?? defaultQr;
  const logo = ("logo" in item ? item.logo : undefined) ?? headerData?.logo;
  const storeName = ("storeName" in item ? item.storeName : undefined) ?? headerData?.store_name;
  const logoDataUrl = "logoDataUrl" in item ? item.logoDataUrl : undefined;

  switch (item.type) {
    case "sale":
      return (
        <SaleReceiptTemplate
          receipt={item.data}
          logo={logo}
          logoDataUrl={logoDataUrl}
          storeName={storeName}
          qr={qr}
          paperWidth={paperWidth}
        />
      );
    case "return_exchange":
      return (
        <ReturnExchangeTemplate
          receipt={item.data}
          logo={logo}
          logoDataUrl={logoDataUrl}
          storeName={storeName}
          qr={qr}
          paperWidth={paperWidth}
        />
      );
    case "z_report":
      return (
        <ZReportTemplate report={item.data} logo={logo} logoDataUrl={logoDataUrl} storeName={storeName} />
      );
    case "cash_movement":
      return (
        <CashMovementTemplate data={item.data} logo={logo} logoDataUrl={logoDataUrl} storeName={storeName} />
      );
    case "estimate":
      return (
        <EstimateTemplate data={item.data} logo={logo} logoDataUrl={logoDataUrl} storeName={storeName} />
      );
  }
}

export function PrintableReceiptPortal() {
  const { t, i18n } = useTranslation("printing");
  const profile = usePrinterProfile();
  const header = useReceiptHeader();
  const activeReceipt = usePrintReceiptStore((s) => s.activeReceipt);
  const activeItem = usePrintReceiptStore((s) => s.activeItem);
  const reprint = usePrintReceiptStore((s) => s.reprint);
  const clear = usePrintReceiptStore((s) => s.clear);
  const sheet = useRef<HTMLDivElement>(null);
  const send = usePrintReceiptImage();
  const sendRef = useRef(send.mutateAsync);
  const isPrintingRef = useRef(false);

  useEffect(() => {
    sendRef.current = send.mutateAsync;
  }, [send.mutateAsync]);

  const itemToPrint: PrintableItem | null = useMemo(
    () => activeItem ?? (activeReceipt ? { type: "sale", data: activeReceipt } : null),
    [activeItem, activeReceipt],
  );

  useEffect(() => {
    if (!itemToPrint || isPrintingRef.current) return;
    isPrintingRef.current = true;

    void (async () => {
      try {
        await document.fonts.ready;
        const node = sheet.current;
        if (!node) return;

        // Ensure all images (e.g. logo) are loaded before rasterizing
        const images = Array.from(node.querySelectorAll("img"));
        await Promise.all(
          images.map((img) =>
            img.complete
              ? Promise.resolve()
              : new Promise((res) => {
                  img.onload = img.onerror = res;
                }),
          ),
        );

        const image = await receiptImageFromElement(node);
        await sendRef.current({
          image,
          orderId: reprint && activeReceipt ? activeReceipt.order_id : null,
        });
        toast.success(t("receiptImage.sent"));
      } catch (e) {
        toast.error(e instanceof CommandError ? translateError(i18n.t, e.error) : t("receiptImage.failed"));
      } finally {
        isPrintingRef.current = false;
        clear();
      }
    })();
  }, [itemToPrint, activeReceipt, reprint, clear, i18n.t, t]);

  if (!itemToPrint) return null;

  const paperWidth = profile.data?.width ?? "mm80";
  const containerWidth = paperWidth === "mm58" ? "48mm" : "72mm";

  return (
    <div
      aria-hidden
      className="pointer-events-none fixed top-0 left-0 -z-50 opacity-0 overflow-hidden"
      style={{ width: containerWidth }}
    >
      <div ref={sheet} style={{ width: containerWidth, backgroundColor: "#ffffff" }}>
        {renderTemplate(itemToPrint, profile.data?.qr, paperWidth, header.data)}
      </div>
    </div>
  );
}
