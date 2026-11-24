import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProductSummary_Serialize as ProductSummary } from "@/db/bindings";
import { playCue } from "@/lib/audio";
import { routeScan, type ScanRoutes } from "./feedback";

vi.mock("@/lib/audio", () => ({ playCue: vi.fn() }));
vi.mock("@/db/bindings", () => ({ commands: { lookupProductCodeMatches: vi.fn() } }));

const MUG = { id: "p1", name: "Mug" } as unknown as ProductSummary;

const SOAP = { id: "p2", name: "Soap" } as unknown as ProductSummary;

function routes(found: ProductSummary[]): ScanRoutes & Record<string, ReturnType<typeof vi.fn>> {
  return {
    onProduct: vi.fn(),
    onMultipleProducts: vi.fn(),
    onOrder: vi.fn(),
    onUnknown: vi.fn(),
    notFound: vi.fn(),
    lookup: vi.fn().mockResolvedValue(found),
  };
}

beforeEach(() => vi.mocked(playCue).mockReset());

describe("routeScan", () => {
  it("adds a known product with the tick", async () => {
    const r = routes([MUG]);
    expect(await routeScan(" 4791234567890 ", r)).toBe("product");
    expect(r.lookup).toHaveBeenCalledWith("4791234567890");
    expect(r.onProduct).toHaveBeenCalledWith(MUG, "4791234567890");
    expect(playCue).toHaveBeenCalledWith("tick");
    expect(r.notFound).not.toHaveBeenCalled();
  });

  it("hands several matches to the picker instead of adding one", async () => {
    const r = routes([MUG, SOAP]);
    expect(await routeScan("4791234567890", r)).toBe("multiple");
    expect(r.onMultipleProducts).toHaveBeenCalledWith([MUG, SOAP], "4791234567890");
    expect(r.onProduct).not.toHaveBeenCalled();
    expect(r.notFound).not.toHaveBeenCalled();
    expect(playCue).toHaveBeenCalledWith("approval");
    expect(playCue).not.toHaveBeenCalledWith("error");
  });

  it("falls back to the first match when no picker is wired", async () => {
    const r = routes([MUG, SOAP]);
    delete r.onMultipleProducts;
    expect(await routeScan("4791234567890", r)).toBe("product");
    expect(r.onProduct).toHaveBeenCalledWith(MUG, "4791234567890");
  });

  it("plays the error tone, toasts and pre-fills Quick Sale for an unknown code", async () => {
    const r = routes([]);
    expect(await routeScan("2000123456781", r)).toBe("unknown");
    expect(playCue).toHaveBeenCalledWith("error");
    expect(r.notFound).toHaveBeenCalledWith("2000123456781");
    expect(r.onUnknown).toHaveBeenCalledWith("2000123456781");
    expect(r.onProduct).not.toHaveBeenCalled();
  });

  it("opens the order for a scanned receipt QR without a product lookup", async () => {
    const r = routes([MUG]);
    const id = "0192F3A4-5B6C-7D8E-9F00-112233445566";
    expect(await routeScan(id, r)).toBe("order");
    expect(r.onOrder).toHaveBeenCalledWith(id.toLowerCase());
    expect(r.lookup).not.toHaveBeenCalled();
  });

  it("opens the exchange for a scanned bill number without a product lookup", async () => {
    const r = { ...routes([MUG]), onBill: vi.fn() };
    expect(await routeScan("t01-261121004", r)).toBe("bill");
    expect(r.onBill).toHaveBeenCalledWith("T01-261121004");
    expect(r.lookup).not.toHaveBeenCalled();
  });
});
