import { describe, expect, it } from "vitest";
import { buildImport, parseCsv } from "./productImport";

describe("parseCsv", () => {
  it("handles quotes, doubled quotes, CRLF, a BOM and blank lines", () => {
    expect(parseCsv(String.fromCharCode(0xfeff) + 'a,b\r\n"x, y","say ""hi"""\r\n\r\n')).toEqual([
      ["a", "b"],
      ["x, y", 'say "hi"'],
    ]);
  });
});

describe("buildImport", () => {
  it("needs name and price headers", () => {
    expect(buildImport("sku,barcode\n1,2\n", true)).toBe("noHeader");
    expect(buildImport("", true)).toBe("noHeader");
  });

  it("builds integer-cent inputs and flags bad rows by line", () => {
    const rows = buildImport(
      "Price,NAME,cost,quantity,sku\n150.50,Soap,110,5,S1\nx,Bad price,,,\n10,,,,\n20,Odd qty,,1.5,\n",
      true,
    );
    if (rows === "noHeader") throw new Error("header expected");
    expect(rows[0]?.input?.price).toBe(15_050);
    expect(rows[0]?.input?.default_cost).toBe(11_000);
    expect(rows[0]?.input?.sku).toEqual({ mode: "manual", value: "S1" });
    expect(rows[0]?.quantity).toBe(5);
    expect(rows.slice(1).map((r) => [r.line, r.issue])).toEqual([
      [3, "priceInvalid"],
      [4, "nameMissing"],
      [5, "quantityInvalid"],
    ]);
  });

  it("ignores the cost column without VIEW_COST", () => {
    const rows = buildImport("name,price,cost\nSoap,10,notmoney\n", false);
    if (rows === "noHeader") throw new Error("header expected");
    expect(rows[0]?.issue).toBeNull();
    expect(rows[0]?.input?.default_cost).toBeNull();
  });
});
