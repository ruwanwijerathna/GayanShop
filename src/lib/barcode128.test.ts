import { describe, expect, it } from "vitest";
import { encodeCode128 } from "./barcode128";

describe("encodeCode128", () => {
  it("encodes numeric data using subset C", () => {
    const widths = encodeCode128("104523");
    expect(widths).not.toBeNull();
    if (!widths) return;
    // 5 symbols (start C + 3 pairs + check) * 11 modules + stop (13 modules) = 68 modules
    const totalModules = widths.reduce((acc, w) => acc + w, 0);
    expect(totalModules).toBe(68);
  });

  it("encodes alphanumeric receipt numbers using subset B", () => {
    const widths = encodeCode128("T01-20261120-0001");
    expect(widths).not.toBeNull();
    if (!widths) return;
    expect(widths.length).toBeGreaterThan(0);
  });

  it("rejects non-ASCII strings", () => {
    expect(encodeCode128("නිල්")).toBeNull();
  });
});
