import { describe, expect, it } from "vitest";
import { bitmapFromRgba, fitLogo, LOGO_MAX_HEIGHT, LOGO_MAX_WIDTH } from "./logoBitmap";

function pixels(values: ([number, number, number, number] | undefined)[]): Uint8ClampedArray {
  return Uint8ClampedArray.from(values.flatMap((v) => v ?? [255, 255, 255, 255]));
}

describe("bitmapFromRgba", () => {
  it("packs dark pixels as set bits, most significant bit first", () => {
    const black: [number, number, number, number] = [0, 0, 0, 255];
    // One 8-wide row: black at x = 0 and x = 7.
    const row = pixels([black, undefined, undefined, undefined, undefined, undefined, undefined, black]);
    expect(bitmapFromRgba(row, 8, 1)).toEqual({ width: 8, height: 1, data: [0b10000001] });
  });

  it("treats transparent pixels as paper and mid-greys by brightness", () => {
    const clear: [number, number, number, number] = [0, 0, 0, 0];
    const darkGrey: [number, number, number, number] = [100, 100, 100, 255];
    const lightGrey: [number, number, number, number] = [160, 160, 160, 255];
    const row = pixels([clear, darkGrey, lightGrey, undefined, undefined, undefined, undefined, undefined]);
    expect(bitmapFromRgba(row, 8, 1).data).toEqual([0b01000000]);
  });

  it("keeps one row per scan line", () => {
    const black: [number, number, number, number] = [0, 0, 0, 255];
    const grid = pixels([
      ...Array<undefined>(8).fill(undefined),
      black,
      ...Array<undefined>(7).fill(undefined),
    ]);
    expect(bitmapFromRgba(grid, 8, 2).data).toEqual([0, 0b10000000]);
  });
});

describe("fitLogo", () => {
  it("never enlarges and rounds the width down to whole bytes", () => {
    expect(fitLogo(100, 50)).toEqual({ width: 96, height: 48 });
  });

  it("scales a wide picture to the paper width, keeping the proportions", () => {
    expect(fitLogo(768, 192)).toEqual({ width: LOGO_MAX_WIDTH, height: 96 });
  });

  it("scales a tall picture to the height limit", () => {
    const fitted = fitLogo(300, 960);
    expect(fitted?.height).toBeLessThanOrEqual(LOGO_MAX_HEIGHT);
    expect((fitted?.width ?? 0) % 8).toBe(0);
  });

  it("rejects an empty picture", () => {
    expect(fitLogo(0, 10)).toBeNull();
  });
});
