// What a scan does on the till (doc §7.1): a known code adds the item (tick), a receipt QR opens its order,
// an unknown code plays the error tone, shows a non-blocking toast and opens Quick Sale pre-filled. Never a
// modal: the next scan must still work. The one exception is a barcode shared by several products (variants
// with the same manufacturer code): the till asks which one, in a keyboard-first dialog.
import { toast } from "sonner";
import { commands, type ProductSummary_Serialize as ProductSummary } from "@/db/bindings";
import { unwrap } from "@/db/client";
import { playCue } from "@/lib/audio";
import { isBillNumber } from "@/lib/receipt";

/** Receipts print a QR of the order id (a UUID). */
const ORDER_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export interface ScanRoutes {
  onProduct: (product: ProductSummary, code: string) => void;
  onOrder: (orderId: string) => void;
  /** A printed bill number (`T01-261121004`) was scanned: look the sale up for an exchange. */
  onBill?: (receiptNo: string) => void;
  /** Unknown code: pre-fill Quick Sale with it. */
  onUnknown: (code: string) => void;
  /** The non-blocking "Not found: …" toast. */
  notFound: (code: string) => void;
  /** Several products share this barcode: let the cashier pick (arrows + Enter). Without it, the first is added. */
  onMultipleProducts?: (products: ProductSummary[], code: string) => void;
  lookup?: (code: string) => Promise<ProductSummary[]>;
}

/** The unknown-barcode toast: red, short-lived, never a modal (`message` is the translated "Not found: …"). */
export function notFoundToast(message: string) {
  toast.error(message, { duration: 4_000 });
}

export type ScanOutcome = "product" | "multiple" | "order" | "bill" | "unknown";

export async function routeScan(code: string, routes: ScanRoutes): Promise<ScanOutcome> {
  const trimmed = code.trim();
  if (ORDER_ID.test(trimmed)) {
    routes.onOrder(trimmed.toLowerCase());
    return "order";
  }
  if (routes.onBill && isBillNumber(trimmed)) {
    playCue("tick");
    routes.onBill(trimmed.toUpperCase());
    return "bill";
  }
  const lookup = routes.lookup ?? ((c: string) => unwrap(commands.lookupProductCodeMatches(c)));
  const [first, ...rest] = await lookup(trimmed);
  if (first && rest.length > 0 && routes.onMultipleProducts) {
    playCue("approval");
    routes.onMultipleProducts([first, ...rest], trimmed);
    return "multiple";
  }
  if (first) {
    playCue("tick");
    routes.onProduct(first, trimmed);
    return "product";
  }
  playCue("error");
  routes.notFound(trimmed);
  routes.onUnknown(trimmed);
  return "unknown";
}
