// Fixtures for label printing in component tests (decision #30). jsdom cannot rasterise HTML, so test files
// mock `@/lib/labelImage` to return a picture of the requested size; these build the sticker layout Rust would
// return for a job, and what `printLabels` then receives.
import type { LabelJob, LabelLayout_Serialize as LabelLayout } from "@/db/bindings";

/** The 50 × 25 mm sticker at 203 dpi. */
export const STICKER = { width: 400, height: 200 } as const;

/** One sticker per job item: shop name, the 8-digit barcode and the price, as `label_preview` lays them out. */
export function stickerLayout(job: LabelJob, warnings: LabelLayout["warnings"] = []): LabelLayout {
  return {
    template_id: job.template_id,
    dpi: 203,
    columns: 2,
    width: STICKER.width,
    height: STICKER.height,
    label_w_mm: 50,
    label_h_mm: 25,
    stickers: job.items.map((i) => ({
      product_id: i.product_id,
      texts: [
        {
          kind: "store_name",
          x: 0,
          y: 32,
          w: 400,
          h: 32,
          font_px: 32,
          bold: true,
          align: "center",
          max_lines: 1,
          text: "Sale Center",
        },
        {
          kind: "price",
          x: 0,
          y: 144,
          w: 400,
          h: 48,
          font_px: 48,
          bold: true,
          align: "center",
          max_lines: 1,
          text: "Rs. 1,250",
        },
      ],
      barcode: {
        symbology: "code128",
        data: "21000423",
        x: 42,
        y: 68,
        bar_h: 36,
        module: 4,
        widths: [2, 1, 1, 2, 2, 2],
        hri_px: 16,
        fits: true,
      },
    })),
    warnings,
  };
}

/** What `printLabels` receives for `job`: each item with the sticker picture drawn for it. */
export function drawnJob(job: LabelJob) {
  return {
    template_id: job.template_id,
    fill_last_row: job.fill_last_row,
    items: job.items.map((i) => ({
      product_id: i.product_id,
      copies: i.copies,
      image: { width: STICKER.width, height: STICKER.height, data: [] },
    })),
  };
}
