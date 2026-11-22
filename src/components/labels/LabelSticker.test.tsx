import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { labelBitmapFromElement } from "@/lib/labelImage";
import { stickerLayout } from "@/test/labels";
import { LabelSticker, LabelStickerPreview } from "./LabelSticker";
import { drawStickers } from "./stickerPrint";

vi.mock("@/lib/labelImage", () => ({
  labelBitmapFromElement: vi.fn((node: HTMLElement, width: number, height: number) =>
    Promise.resolve({ width, height, data: [node.isConnected ? 1 : 0] }),
  ),
}));

function firstSticker(layout: ReturnType<typeof stickerLayout>) {
  const sticker = layout.stickers[0];
  if (!sticker) throw new Error("the fixture has a sticker");
  return sticker;
}

const job = {
  template_id: "two_up_50x25",
  items: [
    { product_id: "p1", copies: 3 },
    { product_id: "p2", copies: 1 },
  ],
  fill_last_row: false,
};

describe("LabelSticker", () => {
  it("is exactly the sticker in dots, with every box where Rust put it", () => {
    const layout = stickerLayout(job);
    const { container } = render(<LabelSticker layout={layout} sticker={firstSticker(layout)} />);
    const sticker = container.firstElementChild as HTMLElement;
    expect([sticker.style.width, sticker.style.height]).toEqual(["400px", "200px"]);
    const price = within(sticker).getByText("Rs. 1,250");
    expect([price.style.left, price.style.top, price.style.width, price.style.fontSize]).toEqual([
      "0px",
      "144px",
      "400px",
      "48px",
    ]);
    const bars = within(sticker).getByTestId("sticker-bars");
    expect([bars.style.left, bars.style.top, bars.getAttribute("height")]).toEqual(["42px", "68px", "36"]);
    // Every bar and space is a whole number of dots: modules × 4.
    const edges = [...bars.querySelectorAll("rect")].flatMap((r) => [
      r.getAttribute("x"),
      r.getAttribute("width"),
    ]);
    expect(edges.length).toBeGreaterThan(0);
    expect(edges.every((v) => /^\d+$/.test(v ?? ""))).toBe(true);
    expect(within(sticker).getByText("21000423")).toBeInTheDocument();
  });

  it("previews the same element scaled to the width asked for", () => {
    const layout = stickerLayout(job);
    render(
      <LabelStickerPreview layout={layout} sticker={firstSticker(layout)} width={200} title="Preview" />,
    );
    const preview = screen.getByRole("img", { name: "Preview" });
    expect([preview.style.width, preview.style.height]).toEqual(["200px", "100px"]);
    expect((preview.firstElementChild as HTMLElement).style.transform).toBe("scale(0.5)");
  });
});

describe("drawStickers", () => {
  it("draws one picture per sticker at the sticker's size, off-screen, and cleans up", async () => {
    const pictures = await drawStickers(stickerLayout(job));
    expect(pictures).toEqual([
      { width: 400, height: 200, data: [1] },
      { width: 400, height: 200, data: [1] },
    ]);
    const calls = vi.mocked(labelBitmapFromElement).mock.calls;
    expect(calls.map(([node]) => node.getAttribute("data-sticker"))).toEqual(["p1", "p2"]);
    expect(document.body.childElementCount).toBe(0);
  });
});
