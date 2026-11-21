import type {
  Logo,
  ReceiptData_Serialize as ReceiptData,
  ZReport_Serialize as ZReportData,
} from "@/db/bindings";

export type { ReceiptData, ZReportData };

export interface CashMovementPrintData {
  kind: "pay_in" | "pay_out" | "no_sale" | string;
  amount: number;
  reason: string;
  staff: string;
  approved_by?: string | null;
  created_at: string;
  business_date?: string;
  store_name?: string;
}

export interface EstimateLine {
  name: string;
  qty: number;
  unit_price: number;
  total: number;
}

export interface EstimatePrintData {
  estimate_no: string;
  created_at: string;
  prepared_by: string;
  customer_name?: string | null;
  customer_phone?: string | null;
  lines: EstimateLine[];
  item_count: number;
  total: number;
  valid_days?: number;
  note?: string;
  store_name?: string;
}

export type PrintableItem =
  | { type: "sale"; data: ReceiptData; logo?: Logo | null; logoDataUrl?: string | null; storeName?: string }
  | {
      type: "return_exchange";
      data: ReceiptData;
      logo?: Logo | null;
      logoDataUrl?: string | null;
      storeName?: string;
    }
  | {
      type: "z_report";
      data: ZReportData;
      logo?: Logo | null;
      logoDataUrl?: string | null;
      storeName?: string;
    }
  | {
      type: "cash_movement";
      data: CashMovementPrintData;
      logo?: Logo | null;
      logoDataUrl?: string | null;
      storeName?: string;
    }
  | {
      type: "estimate";
      data: EstimatePrintData;
      logo?: Logo | null;
      logoDataUrl?: string | null;
      storeName?: string;
    };
