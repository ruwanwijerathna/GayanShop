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

  it("parses the system exported CSV columns (default_cost, on_hand, category, etc.)", () => {
    const csv =
      "sku,barcode,name,name_si,category,price,default_cost,track_stock,on_hand,active\n" +
      "DSI,21000027,dsi,,DSI,3099.00,2603.00,yes,35,yes\n" +
      "PEO525-S12,21000034,DSI,,DSI,3099.00,2603.00,yes,40,yes\n";
    const rows = buildImport(csv, true);
    if (rows === "noHeader") throw new Error("header expected");
    expect(rows).toHaveLength(2);
    expect(rows[0]?.name).toBe("dsi");
    expect(rows[0]?.categoryName).toBe("DSI");
    expect(rows[0]?.quantity).toBe(35);
    expect(rows[0]?.input?.price).toBe(309_900);
    expect(rows[0]?.input?.default_cost).toBe(260_300);
    expect(rows[0]?.input?.sku).toEqual({ mode: "manual", value: "DSI" });
    expect(rows[0]?.input?.barcode).toEqual({ mode: "manual", value: "21000027" });
    expect(rows[0]?.input?.track_stock).toBe(true);
    expect(rows[0]?.input?.is_active).toBe(true);

    expect(rows[1]?.name).toBe("DSI");
    expect(rows[1]?.categoryName).toBe("DSI");
    expect(rows[1]?.quantity).toBe(40);
    expect(rows[1]?.input?.price).toBe(309_900);
    expect(rows[1]?.input?.default_cost).toBe(260_300);
    expect(rows[1]?.input?.sku).toEqual({ mode: "manual", value: "PEO525-S12" });
    expect(rows[1]?.input?.barcode).toEqual({ mode: "manual", value: "21000034" });
  });
});
