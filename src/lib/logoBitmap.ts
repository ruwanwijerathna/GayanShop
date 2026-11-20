// The receipt header image: a picture is scaled to fit the printer's paper and thresholded to the 1-bit
// raster the printer takes (`Logo` in bindings: `width / 8` bytes per row, most significant bit first, 1 = black).
import type { Logo } from "@/db/bindings";

/** Mirrors `LOGO_MAX_WIDTH` / `LOGO_MAX_HEIGHT` in `hardware::escpos`. */
export const LOGO_MAX_WIDTH = 576;
export const LOGO_MAX_HEIGHT = 480;

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

/** Maximum dimensions that fit in the printable area while maintaining aspect ratio. */
export function maxLogoDimensions(srcW: number, srcH: number): { width: number; height: number } | null {
  if (srcW <= 0 || srcH <= 0) return null;
  const scale = Math.min(LOGO_MAX_WIDTH / srcW, LOGO_MAX_HEIGHT / srcH);
  const width = Math.max(8, Math.floor((srcW * scale) / 8) * 8);
  const height = Math.max(1, Math.min(LOGO_MAX_HEIGHT, Math.round(srcH * (width / srcW))));
  return { width, height };
}

/** The scaled size for a given zoom percentage (10% to 100%). */
export function scaleLogoDimensions(
  srcW: number,
  srcH: number,
  percent: number,
): { width: number; height: number } | null {
  const maxDim = maxLogoDimensions(srcW, srcH);
  if (!maxDim) return null;
  const p = Math.max(10, Math.min(100, percent)) / 100;
  const rawW = maxDim.width * p;
  const width = Math.max(8, Math.floor(rawW / 8) * 8);
  const height = Math.max(1, Math.min(LOGO_MAX_HEIGHT, Math.round(srcH * (width / srcW))));
  return { width, height };
}

/** Render a source image or canvas into a 1-bit monochrome `Logo` at a specific scale percentage. */
export function renderScaledLogo(
  source: CanvasImageSource,
  srcW: number,
  srcH: number,
  percent: number,
): Logo | null {
  const size = scaleLogoDimensions(srcW, srcH, percent);
  if (!size) return null;
  const canvas = document.createElement("canvas");
  canvas.width = size.width;
  canvas.height = size.height;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return null;
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, size.width, size.height);
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(source, 0, 0, size.width, size.height);
  return bitmapFromRgba(ctx.getImageData(0, 0, size.width, size.height).data, size.width, size.height);
}

/** Reconstruct an HTMLCanvasElement from an existing `Logo` bitmap. */
export function canvasFromLogo(logo: Logo): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = logo.width;
  canvas.height = logo.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return canvas;
  const image = ctx.createImageData(logo.width, logo.height);
  const rowBytes = logo.width / 8;
  for (let y = 0; y < logo.height; y++) {
    for (let x = 0; x < logo.width; x++) {
      const black = ((logo.data[y * rowBytes + (x >> 3)] ?? 0) & (0x80 >> (x & 7))) !== 0;
      const at = (y * logo.width + x) * 4;
      const v = black ? 0 : 255;
      image.data[at] = v;
      image.data[at + 1] = v;
      image.data[at + 2] = v;
      image.data[at + 3] = 255;
    }
  }
  ctx.putImageData(image, 0, 0);
  return canvas;
}

/** Decode a picture file and convert it; `null` when the browser can't read it as an image. */
export async function logoFromFile(file: File): Promise<Logo | null> {
  const res = await logoFromFileWithSource(file, 100);
  return res ? res.logo : null;
}

/** Decode a picture file, returning the 1-bit `Logo`, dimensions, and a data URL for persistent zooming. */
export async function logoFromFileWithSource(
  file: File,
  percent = 100,
): Promise<{ logo: Logo; dataUrl: string; width: number; height: number } | null> {
  let image: ImageBitmap;
  try {
    image = await createImageBitmap(file);
  } catch {
    return null;
  }
  const logo = renderScaledLogo(image, image.width, image.height, percent);
  const width = image.width;
  const height = image.height;
  image.close();
  if (!logo) return null;

  const dataUrl = await new Promise<string>((resolve) => {
    const reader = new FileReader();
    reader.onload = () => resolve(typeof reader.result === "string" ? reader.result : "");
    reader.onerror = () => resolve("");
    reader.readAsDataURL(file);
  });

  return { logo, dataUrl, width, height };
}
