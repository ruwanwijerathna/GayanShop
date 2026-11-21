import type { Logo, ReceiptData_Serialize as ReceiptData } from "@/db/bindings";
import { ReturnExchangeTemplate, SaleReceiptTemplate } from "./templates";

export interface SinhalaReceiptTemplateProps {
  receipt: ReceiptData;
  logo?: Logo | null;
  logoDataUrl?: string | null;
  storeName?: string;
}

/**
 * Universal Sinhala Receipt Template using FM Gemunu font.
 * Automatically selects either SaleReceiptTemplate or ReturnExchangeTemplate
 * based on whether returns or exchange credits exist on the receipt.
 */
export function SinhalaReceiptTemplate({
  receipt,
  logo,
  logoDataUrl,
  storeName,
}: SinhalaReceiptTemplateProps) {
  const isReturnExchange = (receipt.returns && receipt.returns.length > 0) || receipt.exchange_credit > 0;

  if (isReturnExchange) {
    return (
      <ReturnExchangeTemplate receipt={receipt} logo={logo} logoDataUrl={logoDataUrl} storeName={storeName} />
    );
  }

  return (
    <SaleReceiptTemplate receipt={receipt} logo={logo} logoDataUrl={logoDataUrl} storeName={storeName} />
  );
}
