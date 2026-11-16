// The receipt header image: a picture is scaled to fit the printer's paper and thresholded to the 1-bit
// raster the printer takes (`Logo` in bindings: `width / 8` bytes per row, most significant bit first, 1 = black).
import type { Logo } from "@/db/bindings";

/** Mirrors `LOGO_MAX_WIDTH` / `LOGO_MAX_HEIGHT` in `hardware::escpos`. */
export const LOGO_MAX_WIDTH = 384;
export const LOGO_MAX_HEIGHT = 240;

/** A pixel prints black when it is opaque enough and darker than mid-grey. */
const DARK = 128;

/** Pack RGBA pixels (width already a multiple of 8) into the printer's bitmap. */
export function bitmapFromRgba(rgba: Uint8ClampedArray, width: number, height: number): Logo {
  const rowBytes = width / 8;
  const data = new Array<number>(rowBytes * height).fill(0);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4;
      const alpha = (rgba[i + 3] ?? 0) / 255;
      // Transparent areas are paper-white: blend onto white before measuring brightness.
      const luma = 0.299 * (rgba[i] ?? 0) + 0.587 * (rgba[i + 1] ?? 0) + 0.114 * (rgba[i + 2] ?? 0);
      const shown = luma * alpha + 255 * (1 - alpha);
      if (shown < DARK) {
        const at = y * rowBytes + (x >> 3);
        data[at] = (data[at] ?? 0) | (0x80 >> (x & 7));
      }
    }
  }
  return { width, height, data };
}

/** The size a picture prints at: as large as fits the limits, never enlarged, width in whole bytes. */
export function fitLogo(srcW: number, srcH: number): { width: number; height: number } | null {
  if (srcW <= 0 || srcH <= 0) return null;
  const scale = Math.min(1, LOGO_MAX_WIDTH / srcW, LOGO_MAX_HEIGHT / srcH);
  const width = Math.max(8, Math.floor((srcW * scale) / 8) * 8);
  const height = Math.max(1, Math.round(srcH * (width / srcW)));
  return { width, height: Math.min(height, LOGO_MAX_HEIGHT) };
}

/** Decode a picture file and convert it; `null` when the browser can't read it as an image. */
export async function logoFromFile(file: File): Promise<Logo | null> {
  let image: ImageBitmap;
  try {
    image = await createImageBitmap(file);
  } catch {
    return null;
  }
  const size = fitLogo(image.width, image.height);
  const canvas = document.createElement("canvas");
  const ctx = size ? canvas.getContext("2d", { willReadFrequently: true }) : null;
  if (!size || !ctx) {
    image.close();
    return null;
  }
  canvas.width = size.width;
  canvas.height = size.height;
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, size.width, size.height);
  ctx.drawImage(image, 0, 0, size.width, size.height);
  image.close();
  return bitmapFromRgba(ctx.getImageData(0, 0, size.width, size.height).data, size.width, size.height);
}
