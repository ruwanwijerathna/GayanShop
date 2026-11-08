import { describe, expect, it } from "vitest";
import { AUDIT_COLUMN_WIDTHS, AUDIT_TABLE_BUDGET } from "./AuditLogScreen";

describe("AuditLogScreen layout", () => {
  it("fits every column at the 1024×768 minimum window without horizontal scroll", () => {
    const total = Object.values(AUDIT_COLUMN_WIDTHS).reduce((a, b) => a + b, 0);
    expect(total).toBeLessThanOrEqual(AUDIT_TABLE_BUDGET);
  });

  it("leaves room for the longest en-LK timestamp (186 px of text + 24 px padding)", () => {
    expect(AUDIT_COLUMN_WIDTHS.when).toBeGreaterThanOrEqual(210);
  });
});
