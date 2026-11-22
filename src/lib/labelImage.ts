// A sticker drawn in the webview, printed as a picture (decision #30). The element is already one CSS pixel per
// printer dot, so it is drawn at a pixel ratio of 1 onto a canvas exactly the sticker's size and thresholded to
// the 1-bit raster the label printer takes (the receipt picture's packing: 1 = black, MSB first).
import { toCanvas } from "html-to-image";
import type { LabelBitmap } from "@/db/bindings";
import { bitmapFromRgba } from "@/lib/logoBitmap";

export async function labelBitmapFromElement(
  node: HTMLElement,
  width: number,
  height: number,
): Promise<LabelBitmap> {
  if (width <= 0 || height <= 0 || width % 8 !== 0) throw new Error("sticker size is not whole bytes");
  const drawn = await toCanvas(node, {
    pixelRatio: 1,
    width,
    height,
    canvasWidth: width,
    canvasHeight: height,
    backgroundColor: "#ffffff",
  });
  // Exactly the sticker's size, on white, so a stray pixel never shifts the bytes.
  const sheet = document.createElement("canvas");
  sheet.width = width;
  sheet.height = height;
  const ctx = sheet.getContext("2d", { willReadFrequently: true });
  if (!ctx) throw new Error("no 2d canvas");
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, width, height);
  ctx.drawImage(drawn, 0, 0);
  const packed = bitmapFromRgba(ctx.getImageData(0, 0, width, height).data, width, height);
  return { width: packed.width, height: packed.height, data: packed.data };
}
