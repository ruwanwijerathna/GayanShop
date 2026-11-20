// A receipt drawn in the webview, printed as a picture: Sinhala text can't go through the printer's own fonts.
// The element is drawn at the paper's full width (576 dots) and thresholded to the 1-bit raster the printer
// takes, the same packing the header image uses.
import { toCanvas } from "html-to-image";
import type { ReceiptImage } from "@/db/bindings";
import { bitmapFromRgba, LOGO_MAX_WIDTH } from "@/lib/logoBitmap";

/** Mirrors `RECEIPT_IMAGE_MAX_HEIGHT` in `hardware::escpos`. */
export const RECEIPT_IMAGE_MAX_HEIGHT = 6000;

/** Draw `node` as wide as the paper and pack it into the printer's bitmap. */
export async function receiptImageFromElement(node: HTMLElement): Promise<ReceiptImage> {
  const shownWidth = node.offsetWidth;
  if (shownWidth <= 0) throw new Error("receipt element has no width");
  const drawn = await toCanvas(node, {
    pixelRatio: LOGO_MAX_WIDTH / shownWidth,
    backgroundColor: "#ffffff",
  });
  const height = drawn.height;
  if (height <= 0 || height > RECEIPT_IMAGE_MAX_HEIGHT)
    throw new Error("receipt picture has an unusable height");

  // Exactly paper width, on white, so a rounding pixel never shifts the bytes.
  const paper = document.createElement("canvas");
  paper.width = LOGO_MAX_WIDTH;
  paper.height = height;
  const ctx = paper.getContext("2d", { willReadFrequently: true });
  if (!ctx) throw new Error("no 2d canvas");
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, LOGO_MAX_WIDTH, height);
  ctx.drawImage(drawn, 0, 0);
  const rgba = ctx.getImageData(0, 0, LOGO_MAX_WIDTH, height).data;
  return bitmapFromRgba(rgba, LOGO_MAX_WIDTH, height);
}
