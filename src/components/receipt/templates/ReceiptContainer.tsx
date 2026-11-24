import type { ReactNode } from "react";
import type { Logo, PaperWidth } from "@/db/bindings";
import { canvasFromLogo } from "@/lib/logoBitmap";

export interface ReceiptContainerProps {
  logo?: Logo | null;
  logoDataUrl?: string | null;
  storeName?: string;
  children: ReactNode;
  className?: string;
  paperWidth?: PaperWidth;
}

export function ReceiptContainer({
  logo,
  logoDataUrl: propLogoUrl,
  storeName = "Gayan Sales Center",
  children,
  className = "",
  paperWidth = "mm80",
}: ReceiptContainerProps) {
  const logoSrc =
    propLogoUrl ??
    (logo ? canvasFromLogo(logo).toDataURL() : null) ??
    (typeof window !== "undefined" ? localStorage.getItem("receipt_logo_source") : null);

  const containerWidth = paperWidth === "mm58" ? "48mm" : "72mm";

  return (
    <div
      className={`receipt-container mx-auto flex flex-col gap-2 bg-white p-3 text-black selection:bg-none ${className}`}
      style={{
        width: containerWidth,
        maxWidth: "100%",
        fontFamily: "'Noto Sans Sinhala', 'Inter', -apple-system, BlinkMacSystemFont, sans-serif",
        fontSize: "12px",
        lineHeight: "1.35",
      }}
    >
      {/* 1. Header Logo (Store Name is inside logo). If no logo, show Store Name */}
      {logoSrc ? (
        <div className="flex justify-center pb-1">
          <img src={logoSrc} alt={storeName} className="max-h-28 w-auto max-w-full object-contain" />
        </div>
      ) : (
        <h1
          className="text-center text-base font-bold tracking-wide"
          style={{ fontFamily: "'Inter', sans-serif" }}
        >
          {storeName}
        </h1>
      )}

      {children}
    </div>
  );
}

/** Formats an ISO date into separate YYYY-MM-DD and HH:MM strings */
export function formatDateTime(iso: string) {
  try {
    const d = new Date(iso);
    if (isNaN(d.getTime())) return { date: iso, time: "" };
    const date = d.toLocaleDateString("en-CA"); // YYYY-MM-DD
    const time = d.toLocaleTimeString("en-US", {
      hour: "2-digit",
      minute: "2-digit",
      hour12: true,
    });
    return { date, time };
  } catch {
    return { date: iso, time: "" };
  }
}
