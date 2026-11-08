import { describe, expect, it } from "vitest";
import { cn } from "./utils";

describe("cn", () => {
  it("joins class names and drops falsy values", () => {
    expect(cn("h-12", false, undefined, "px-4")).toBe("h-12 px-4");
  });

  it("lets later Tailwind utilities win over conflicting earlier ones", () => {
    expect(cn("h-9 px-2", "h-12")).toBe("px-2 h-12");
  });
});
