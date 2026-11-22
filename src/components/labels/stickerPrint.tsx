// Printing stickers (decision #30): Rust lays each product's sticker out in dots, the webview draws it
// off-screen in HTML/CSS (`LabelSticker`), turns each into the printer's 1-bit picture and hands the pictures
// back to Rust, which places them on the roll's columns and queues the job for the label printer.
//
// The drawing is a short-lived React root of its own, not a portal mounted somewhere in the app, so printing
// works from anywhere (a dialog that has already closed, the receive flow, Settings) and every caller simply
// awaits the result: a CommandError such as "needs a supervisor's approval" comes back to it.
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import type { LabelBitmap, LabelJob, LabelLayout_Serialize as LabelLayout } from "@/db/bindings";
import { labelPrinting } from "@/db/hooks";
import { labelBitmapFromElement } from "@/lib/labelImage";
import { LabelSticker } from "./LabelSticker";

/** Draw every sticker of a layout off-screen and return their pictures, in layout order. */
export async function drawStickers(layout: LabelLayout): Promise<LabelBitmap[]> {
  const host = document.createElement("div");
  host.setAttribute("aria-hidden", "true");
  host.style.cssText = "position:fixed;top:0;left:-10000px;pointer-events:none";
  document.body.append(host);
  const root = createRoot(host);
  try {
    flushSync(() =>
      root.render(
        <>
          {layout.stickers.map((s, i) => (
            <LabelSticker key={`${s.product_id}:${i}`} layout={layout} sticker={s} />
          ))}
        </>,
      ),
    );
    // The bundled fonts must be in before the picture is taken, or the text is drawn in a fallback.
    await document.fonts?.ready;
    const nodes = Array.from(host.children).filter((n): n is HTMLElement => n instanceof HTMLElement);
    const pictures: LabelBitmap[] = [];
    for (const node of nodes) pictures.push(await labelBitmapFromElement(node, layout.width, layout.height));
    return pictures;
  } finally {
    root.unmount();
    host.remove();
  }
}

/** Lay out, draw and queue a label job; resolves with the number of stickers queued. */
export async function printLabelJob(job: LabelJob, approvalId: string | null): Promise<number> {
  const layout = await labelPrinting.layout(job);
  const pictures = await drawStickers(layout);
  await labelPrinting.print(
    {
      template_id: job.template_id,
      fill_last_row: job.fill_last_row,
      items: job.items.map((item, i) => {
        const image = pictures[i];
        if (!image) throw new Error("a sticker was not drawn");
        return { product_id: item.product_id, copies: item.copies, image };
      }),
    },
    approvalId,
  );
  return job.items.reduce((n, i) => n + i.copies, 0);
}

/** Settings → "Print test label": draw the sample sticker and print it across one row of the roll. */
export async function printSampleLabel(templateId: string): Promise<void> {
  const layout = await labelPrinting.sampleLayout(templateId);
  const [image] = await drawStickers(layout);
  if (!image) throw new Error("the sample sticker was not drawn");
  await labelPrinting.printSample(templateId, image);
}
